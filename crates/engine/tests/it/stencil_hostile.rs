//! Stencil files made to cost: crafted, and held to a hang guard of a minute (the counts of their work are in the unit tests of
//! `stencil/bring`, `stencil/refs` and `stencil/instance`). A clean fuzz run says nothing about time (the reader hung for minutes on a
//! file of under a megabyte, inside a dependency, before any limit of ours ran), so each shape a stencil can take that costs work is built in
//! code and timed here, and a bound is charged to bytes and nodes and not to calls. A bound proved here holds for the fixture measured: what
//! each fixture lacks is said at its test. The times print with `--nocapture`.

use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::stencil_open::{CT_STENCIL, Dir, base64, box_cells, ids, open};
use crate::{blank_session, check_session, run};

/// The most time any one of these may take, in the development profile on a loaded machine: a hang, not a speed test. The measured times are
/// a tenth of it or less.
const HANG: Duration = Duration::from_secs(60);

fn timed<T>(what: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    println!("{what}: {took:?}");
    assert!(took < HANG, "{what} took {took:?}");
    (out, took)
}

fn tiny(id: u32, name: &str) -> MasterSpec {
    MasterSpec::new(id, name).attrs(&ids(id, id + 1_000_000)).shapes(&shape(5, &box_cells(1.0, 1.0)))
}

#[test]
fn thousands_of_masters_open_list_and_drop_in_time() {
    let dir = Dir::new();
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for id in 1..=5_000 {
        builder = builder.master(tiny(id, &format!("Master {id}")));
    }
    let bytes = builder.build();
    println!("5,000 masters: {} bytes", bytes.len());
    let path = dir.write("many.vssx", &bytes);
    let mut s = blank_session();
    let (opened, _) = timed("open 5,000 masters", || open(&mut s, &path));
    // All 5,000 are in the stencil; the listing holds the first 2,048 and says how many there are, so one answer stays a few hundred kilobytes.
    assert_eq!(opened["masters_total"], 5_000);
    assert_eq!(opened["masters"].as_array().unwrap().len(), 2_048);
    let (list, _) = timed("list", || run(&mut s, "stencil.list", json!({})));
    let size = list.to_string().len();
    println!("list answer: {size} bytes");
    assert!(size < 1_000_000, "{size}");
    // A master past the listing drops by its id and by its name.
    let (dropped, _) =
        timed("drop master 4,999", || run(&mut s, "shape.drop", json!({"master": "4999", "stencil": "file-1", "page": 1, "x": 3, "y": 3})));
    assert_eq!(dropped["master_added"], true);
    run(&mut s, "shape.drop", json!({"master": "Master 5000", "stencil": "file-1", "page": 1, "x": 5, "y": 3}));
    // Three hundred different masters into one drawing: each is a new master, a search of the drawing's masters, a recalculation.
    let (_, _) = timed("drop 300 masters", || {
        for id in 1..=300 {
            run(
                &mut s,
                "shape.drop",
                json!({"master": id.to_string(), "stencil": "file-1", "page": 1, "x": 1.0 + f64::from(id % 9), "y": 1.0 + f64::from(id / 40)}),
            );
        }
    });
    assert_eq!(s.document().unwrap().masters.len(), 302);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn many_masters_with_one_unique_id_are_listed_and_each_drops_as_itself() {
    let dir = Dir::new();
    // What the fixture lacks: masters that differ in more than their size.
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for id in 1..=3_000 {
        let w = 1.0 + f64::from(id % 7);
        builder = builder.master(MasterSpec::new(id, &format!("Twin {id}")).attrs(&ids(7, 7)).shapes(&shape(5, &box_cells(w, 1.0))));
    }
    let mut s = blank_session();
    let (_, _) = timed("open 3,000 masters with one UniqueID", || open(&mut s, &dir.write("twins.vssx", &builder.build())));
    let a = run(&mut s, "shape.drop", json!({"master": "Twin 1", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let b = run(&mut s, "shape.drop", json!({"master": "Twin 2", "stencil": "file-1", "page": 1, "x": 3, "y": 6}));
    assert_ne!(a["master"], b["master"]);
    assert_eq!((a["width"].as_f64(), b["width"].as_f64()), (Some(2.0), Some(3.0)));
    let again = run(&mut s, "shape.drop", json!({"master": "Twin 1", "stencil": "file-1", "page": 1, "x": 6, "y": 3}));
    assert_eq!(again["master"], a["master"], "an ambiguous master is reused when what it would add is what the drawing has");
    assert_eq!(s.document().unwrap().masters.len(), 2);
}

/// A master of a group with `n` members, each `<Shape ID="k"/>` with `member_attrs`, and the part's size.
fn big_master(n: u32, member_attrs: &str) -> (MasterSpec, usize) {
    let mut members = String::new();
    for k in 0..n {
        members.push_str(&format!("<Shape ID=\"{}\"{member_attrs}/>", 10 + k));
    }
    let group = shape_with(5, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(2.0, 2.0)));
    let size = group.len();
    (MasterSpec::new(1, "Big").shapes(&group), size)
}

#[test]
fn a_master_of_a_megabyte_of_shapes_opens_and_drops_in_time() {
    let dir = Dir::new();
    // What the fixture lacks: members with cells of their own (here none), and a deep group (see the next test).
    let (master, size) = big_master(70_000, "");
    println!("master part: {size} bytes");
    assert!(size > 1_000_000);
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(master).build();
    let mut s = blank_session();
    let (opened, _) = timed("open", || open(&mut s, &dir.write("big.vssx", &bytes)));
    assert_eq!(opened["masters"][0]["width"], 2.0);
    let (r, _) = timed("drop", || run(&mut s, "shape.drop", json!({"master": "Big", "stencil": "file-1", "page": 1, "x": 3, "y": 3})));
    let id = r["id"].as_u64().unwrap() as u32;
    let page = s.document().unwrap().page(1).unwrap();
    assert_eq!(page.shapes.len(), 1, "the instance leaves its members to the master");
    let inst = page.shapes.get(0).unwrap();
    assert_eq!((inst.id, inst.kind.text()), (id, Some("Group")));
    assert_eq!(inst.children.len(), 0, "70,000 members are past what an instance lists");
    let (_, _) = timed("drop again", || run(&mut s, "shape.drop", json!({"master": "Big", "stencil": "file-1", "page": 1, "x": 7, "y": 3})));
    assert_eq!(s.document().unwrap().masters.len(), 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_master_whose_members_are_all_formulas_drops_in_time_listed_or_left_to_the_master() {
    let dir = Dir::new();
    // Every member reads its parent's size and its own pin: the cells a drop writes (`PinX`, `PinY` of the instance) are read by a formula of each
    // member by `Sheet.5!` and `PinX`, so the recalculation of what reads them has all of them to look at. What the fixture lacks: formulas that
    // read other members, and long ones.
    let member = r#"<Cell N="Width" V="1" F="Sheet.5!Width*0.5"/><Cell N="Height" V="1" F="Sheet.5!Height*0.5"/><Cell N="PinX" V="1" F="Width*0.5+Sheet.5!PinX*0"/><Cell N="PinY" V="1" F="Height*0.5"/>"#;
    for (n, listed) in [(4_000_u32, true), (30_000, false)] {
        let mut members = String::new();
        for k in 0..n {
            members.push_str(&format!("<Shape ID=\"{}\">{member}</Shape>", 10 + k));
        }
        let group = shape_with(5, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(2.0, 2.0)));
        let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Formulas").shapes(&group)).build();
        let mut s = blank_session();
        timed(&format!("open {n} members with formulas"), || open(&mut s, &dir.write(&format!("f{n}.vssx"), &bytes)));
        let (_, _) = timed(&format!("drop {n} members with formulas"), || {
            run(&mut s, "shape.drop", json!({"master": "Formulas", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))
        });
        let (_, _) = timed("drop again", || run(&mut s, "shape.drop", json!({"master": "Formulas", "stencil": "file-1", "page": 1, "x": 7, "y": 3})));
        let inst = s.document().unwrap().page(1).unwrap().shapes.get(0).unwrap().clone();
        assert_eq!(inst.children.len() == n as usize, listed);
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_master_whose_every_shape_needs_a_style_or_colour_rewritten_copies_in_time() {
    let dir = Dir::new();
    // The worst case for the copy: no shape can be shared with the stencil, since each one's links and colours change. The drawing has a
    // style 3 of its own, so the stencil's goes under a free ID, and a colour 25 of its own.
    let cells = r#"<Cell N="FillForegnd" V="25"/>"#;
    let mut members = String::new();
    for k in 0..40_000u32 {
        members.push_str(&format!("<Shape ID=\"{}\" LineStyle=\"3\" FillStyle=\"3\" TextStyle=\"3\">{cells}</Shape>", 10 + k));
    }
    let group = shape_with(5, r#" Type="Group" LineStyle="3""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(2.0, 2.0)));
    let big = MasterSpec::new(1, "Heavy").shapes(&group);
    println!("master part: {} bytes", group.len());
    let stencil = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .colors(crate::stencil_open::STENCIL_COLORS)
        .more_styles(crate::stencil_open::STENCIL_STYLES)
        .master(big)
        .build();
    let target = VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="25" RGB="#112233"/>"##)
        .more_styles(r#"<StyleSheet ID="3" NameU="Mine" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.05"/></StyleSheet>"#)
        .page(diacraft_testkit::package::PageSpec::new(1, "Page-1"))
        .build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("heavy.vssx", &stencil));
    let (_, _) = timed("drop and rewrite 40,000 shapes", || {
        run(&mut s, "shape.drop", json!({"master": "Heavy", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))
    });
    let d = s.document().unwrap();
    let m = d.masters.iter().next().unwrap();
    let top = m.shapes.get(0).unwrap();
    let member = top.children.get(39_999).unwrap();
    let flow = d.styles.iter().find(|st| st.name_u.as_deref() == Some("Flow Normal")).unwrap();
    assert_eq!(member.sheet.links.line, Some(flow.id));
    assert_ne!(flow.id, 3);
    let colour = member.sheet.cells.get("FillForegnd").and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!(d.color(colour as i64) == diacraft_color::Rgba::from_hex("#3366CC"), "the member's colour is the stencil's, now at another number");
    // The stencil's own master is as it was: the copy did not write through a shared shape.
    let again = run(&mut s, "shape.drop", json!({"master": "Heavy", "stencil": "file-1", "page": 1, "x": 6, "y": 3}));
    assert_eq!(again["master_added"], false);
    let stencil = s.stencils().get("file-1").unwrap().document().masters.get(0).unwrap().shapes.get(0).unwrap().children.get(39_999).unwrap().clone();
    assert_eq!(stencil.sheet.links.line, Some(3), "the stencil's shapes keep their links");
    // The drawing saves and reads back with the 40,000 members of the master and its styles.
    let path = dir.path("heavy-out.vsdx");
    let (_, _) = timed("save", || run(&mut s, "file.save_as", json!({"path": path})));
    let mut t = Session::new();
    let (_, _) = timed("reopen", || run(&mut t, "file.open", json!({"path": path})));
    let e = t.document().unwrap();
    assert_eq!(e.masters.iter().next().unwrap().shapes.get(0).unwrap().children.len(), 40_000);
    assert_eq!(e.styles.len(), s.document().unwrap().styles.len());
}

#[test]
fn masters_that_name_each_other_and_shapes_that_name_themselves_do_not_loop() {
    let dir = Dir::new();
    // A names B, B names A, in `Master` attributes of master shapes (a master part holds none), in `MasterShape`, and in `USE()` formulas;
    // a cell reads itself through its sheet and a master shape reads a shape of the other master. None of it is followed.
    let a = MasterSpec::new(1, "A").attrs(&ids(1, 11)).shapes(&shape_with(
        5,
        r#" Master="2" MasterShape="5""#,
        &format!(
            r#"{}<Cell N="FillPattern" V="1" F="USE(&quot;B&quot;)"/><Cell N="LineWeight" V="0.01" F="Sheet.5!LineWeight"/>"#,
            box_cells(1.0, 1.0)
        ),
    ));
    let b = MasterSpec::new(2, "B").attrs(&ids(2, 12)).shapes(&shape_with(
        5,
        r#" Master="1" MasterShape="5""#,
        &format!(r#"{}<Cell N="FillPattern" V="1" F="USE(&quot;A&quot;)"/><Cell N="Width" V="1" F="Sheet.5!Height"/>"#, box_cells(1.0, 1.0)),
    ));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(a).master(b).build();
    let mut s = blank_session();
    timed("open", || open(&mut s, &dir.write("loop.vssx", &bytes)));
    let (r, _) = timed("drop A", || run(&mut s, "shape.drop", json!({"master": "A", "stencil": "file-1", "page": 1, "x": 3, "y": 3})));
    run(&mut s, "shape.drop", json!({"master": "B", "stencil": "file-1", "page": 1, "x": 6, "y": 3}));
    let d = s.document().unwrap();
    assert_eq!(d.masters.len(), 2, "each is copied alone: a name in a formula does not bring the master it names");
    let notes = r["notes"].as_array().expect("notes").iter().map(|n| n.as_str().unwrap().to_owned()).collect::<Vec<_>>();
    assert!(notes.iter().any(|n| n.contains("USE")), "the dropped master says its formulas name other masters: {notes:?}");
    assert_eq!(check_session(&s), Ok(()));
    // The drop of each is rendered: nothing in it loops.
    let view = diacraft_render::View::fit_page(d, 1, 50.0).unwrap();
    let (_, _) = timed("render", || diacraft_render::Renderer::new().render(d, 1, &view));
}

#[test]
fn thousands_of_masters_of_sixty_four_top_level_shapes_open_in_time_and_the_sizing_is_bounded() {
    let dir = Dir::new();
    // 4,000 masters of 64 shapes is 256,000 shapes; the listing sizes at most 8 of a master's shapes and 10,000 shapes of a stencil, so the masters
    // past that are listed with no size (they still drop: the drop reads the size from the drawing's copy). What the fixture lacks: shapes with
    // long chains of styles to read a size through, which is the resolver's own bound (64 steps).
    let shapes: String = (0..64_u32).map(|k| shape(10 + k, r#"<Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#)).collect();
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for id in 1..=4_000_u32 {
        builder = builder.master(MasterSpec::new(id, &format!("Crowd {id}")).attrs(&ids(id, id + 10_000)).shapes(&shapes));
    }
    let bytes = builder.build();
    println!("4,000 masters of 64 shapes: {} bytes", bytes.len());
    let mut s = blank_session();
    let (opened, _) = timed("open", || open(&mut s, &dir.write("crowds.vssx", &bytes)));
    let masters = opened["masters"].as_array().unwrap();
    assert!(masters.first().unwrap()["width"].as_f64().unwrap() > 0.0, "the first masters are sized");
    assert_eq!(masters.get(2_047).unwrap()["width"].as_f64(), Some(0.0), "the share of a stencil is spent by then");
    let (r, _) =
        timed("drop a master of 64 shapes", || run(&mut s, "shape.drop", json!({"master": "4000", "stencil": "file-1", "page": 1, "x": 3, "y": 3})));
    assert_eq!(r["width"], 1.0, "the drop reads its size from the master it drops");
}

#[test]
fn a_chain_of_alike_named_styles_dropped_into_a_drawing_of_as_many_costs_a_hash_each() {
    let dir = Dir::new();
    // The review's fixture, scaled down: a chain of 20,000 styles all called "Same" in the stencil, a drawing with 30,000 styles all called "Same" too
    // (none the same as the chain's), so that a search of the drawing's styles of the name for each style of the chain is 20,000 times 30,000 sheet
    // comparisons (6e8). The count of comparisons is `bring/tests.rs`'s; this is the whole drop, in time.
    let (n, m) = (20_000_u32, 30_000_u32);
    let chain: String = (1..=n)
        .map(|k| {
            let next = if k == n { 0 } else { k + 1 };
            format!(
                "<StyleSheet ID=\"{k}\" NameU=\"Same\" Name=\"Same\" LineStyle=\"{next}\" FillStyle=\"{next}\" TextStyle=\"{next}\"><Cell N=\"LineWeight\" V=\"{}\"/></StyleSheet>",
                0.01 + f64::from(k) * 1e-6
            )
        })
        .collect();
    let others: String = (1..=m)
        .map(|k| format!("<StyleSheet ID=\"{k}\" NameU=\"Same\" Name=\"Same\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Cell N=\"LineWeight\" V=\"{}\"/></StyleSheet>", 7.0 + f64::from(k)))
        .collect();
    let master = MasterSpec::new(1, "Deep").shapes(&shape_with(5, r#" LineStyle="1" FillStyle="1" TextStyle="1""#, &box_cells(1.0, 1.0)));
    let stencil = VsdxBuilder::new().document_type(CT_STENCIL).more_styles(&chain).master(master).build();
    let target = VsdxBuilder::new().more_styles(&others).page(diacraft_testkit::package::PageSpec::new(1, "Page-1")).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("same.vssx", &stencil));
    let (r, _) = timed("drop a chain of 20,000 alike-named styles into 30,000", || {
        run(&mut s, "shape.drop", json!({"master": "Deep", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))
    });
    assert_eq!(r["copied"]["styles"], 20_000);
    assert_eq!(s.document().unwrap().styles.len(), 1 + 30_000 + 20_000);
    let (again, _) = timed("drop again", || run(&mut s, "shape.drop", json!({"master": "Deep", "stencil": "file-1", "page": 1, "x": 6, "y": 3})));
    assert_eq!(again["master_added"], false);
}

#[test]
fn styles_in_a_long_chain_are_followed_without_recursion() {
    let dir = Dir::new();
    // Style k is based on k + 1, twenty thousand deep (a real file reaches the root in five). The copy walks the chain without a stack.
    let n = 20_000u32;
    let mut styles = String::new();
    for k in 1..=n {
        let next = if k == n { 0 } else { k + 1 };
        styles.push_str(&format!(
            "<StyleSheet ID=\"{k}\" NameU=\"S{k}\" LineStyle=\"{next}\" FillStyle=\"{next}\" TextStyle=\"{next}\"><Cell N=\"LineWeight\" V=\"{}\"/></StyleSheet>",
            0.01 + f64::from(k) * 1e-6
        ));
    }
    let m = MasterSpec::new(1, "Deep").shapes(&shape_with(5, r#" LineStyle="1" FillStyle="1" TextStyle="1""#, &box_cells(1.0, 1.0)));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).more_styles(&styles).master(m).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("deep.vssx", &bytes));
    let (r, _) = timed("drop with a chain of 20,000 styles", || {
        run(&mut s, "shape.drop", json!({"master": "Deep", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))
    });
    assert_eq!(r["master_added"], true);
    assert_eq!(s.document().unwrap().styles.len(), 1 + n as usize);
    let (again, _) = timed("drop again", || run(&mut s, "shape.drop", json!({"master": "Deep", "stencil": "file-1", "page": 1, "x": 6, "y": 3})));
    assert_eq!(again["master_added"], false);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_icon_of_huge_declared_size_costs_nothing_to_list_and_is_refused_when_asked_for() {
    let dir = Dir::new();
    // `IconSize` is the file's own word and means nothing here; the text is 12 MiB of base64 letters. What the fixture lacks: thousands of such icons
    // (the reader's limit on the part's size is what bounds them).
    let huge = "A".repeat(12 * 1024 * 1024);
    let small = base64(&[0, 0, 1, 0, 1, 0, 16, 16]);
    let bytes = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(tiny(1, "Huge").attrs(&format!("{} IconSize=\"4294967295\"", ids(1, 2))).icon(&huge))
        .master(tiny(2, "Small").icon(&small))
        .build();
    println!("file: {} bytes", bytes.len());
    let mut s = blank_session();
    let (opened, _) = timed("open", || open(&mut s, &dir.write("icons.vssx", &bytes)));
    assert_eq!(opened["masters"][0]["icon"], Value::Null);
    assert_eq!(opened["masters"][0]["icon_refused"], "too large");
    assert_eq!(opened["masters"][1]["icon"]["format"], "ico");
    assert!(opened.to_string().len() < 10_000);
    let (msg, _) = timed("refuse", || s.execute("stencil.icon", &json!({"stencil": "file-1", "master": "1"})).unwrap_err().to_string());
    assert!(msg.contains("too large"), "{msg}");
    run(&mut s, "shape.drop", json!({"master": "Huge", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let kept = s.document().unwrap().masters.iter().next().unwrap().icon.as_ref().map(|i| i.len());
    assert_eq!(kept, Some(12 * 1024 * 1024), "the copy keeps the icon text as the file had it, shared and not copied");
}

#[test]
fn a_stencil_with_a_colour_table_and_face_names_in_the_thousands_copies_only_what_a_master_uses() {
    let dir = Dir::new();
    let mut colors = String::new();
    for ix in 24..=253u32 {
        colors.push_str(&format!("<ColorEntry IX=\"{ix}\" RGB=\"#{:06X}\"/>", 0x10_0000 + ix * 7));
    }
    let mut faces = String::new();
    for k in 0..3_000 {
        faces.push_str(&format!("<FaceName NameU=\"Face {k}\"/>"));
    }
    let inner = format!(
        r#"{}<Cell N="FillForegnd" V="100"/><Section N="Character"><Row IX="0"><Cell N="Font" V="Face 2999"/></Row></Section>"#,
        box_cells(1.0, 1.0)
    );
    let bytes = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .colors(&colors)
        .face_names(&faces)
        .master(MasterSpec::new(1, "Picky").shapes(&shape(5, &inner)))
        .build();
    let mut s = blank_session();
    open(&mut s, &dir.write("picky.vssx", &bytes));
    run(&mut s, "shape.drop", json!({"master": "Picky", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let d = s.document().unwrap();
    assert_eq!(d.colors.len(), 1);
    assert_eq!(d.fonts.iter().map(|f| f.name_u.to_string()).collect::<Vec<_>>(), ["Face 2999"]);
}
