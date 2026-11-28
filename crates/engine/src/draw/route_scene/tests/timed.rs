//! Timed twins of the counted tests (ignored: a time moves with the load on the machine, and no test asserts one in the gate). Each prints its time beside a **yardstick**, the plain work the
//! same page costs to read: a pass over the page's top-level shapes reading forty cells of each through the model's own reader, which is what reading a shape's box and flags comes to.
//! Run by hand with `cargo nextest run -p diacraft-engine --lib --run-ignored only --no-capture -E 'test(/route_scene::tests::timed::/)'`, with `uptime` under 32.

use std::time::{Duration, Instant};

use diacraft_doc::{Document, Owner, ShapeRef, SheetRef};

use super::build::{boxed, nudge_at, page, shift};
use super::plenty;
use crate::draw::read::Reader;
use crate::draw::route_scene::Scene;

/// The yardstick: forty cells of each top-level shape of page 1, read through the model's reader.
fn yardstick(doc: &Document) -> Duration {
    let started = Instant::now();
    let reader = Reader::new(doc);
    let mut sum = 0.0;
    for at in 0..doc.page(1).map_or(0, |p| p.shapes.len()) {
        let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![u32::try_from(at).unwrap_or(0)] });
        for name in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "ObjType"].iter().cycle().take(40) {
            sum += reader.number(&sheet, name);
        }
    }
    std::hint::black_box(sum);
    started.elapsed()
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let out = f();
    (out, started.elapsed())
}

fn boxes(n: u32) -> Document {
    page((1..=n).map(|k| boxed(k, (f64::from(k % 200) * 3.0, f64::from(k / 200) * 3.0, 1.0, 1.0)).cell("ObjType", "1")).collect(), Vec::new())
}

#[test]
#[ignore = "a timing: run by hand beside the yardstick"]
fn the_first_read_a_second_put_right_and_an_edit_of_every_shape_beside_the_yardstick() {
    let mut doc = boxes(10_000);
    let plain = yardstick(&doc);
    let (mut scene, first) = timed(|| Scene::read(&doc, 1, &mut plenty()));
    shift(&mut doc, 4_321, 0.0, 0.25);
    let (_, second) = timed(|| scene.put_right(&doc, 1, &mut plenty()));
    for at in 0..10_000 {
        nudge_at(&mut doc, at, 0.25);
    }
    let (_, every) = timed(|| scene.put_right(&doc, 1, &mut plenty()));
    eprintln!("TIMED yardstick {plain:?}; first read {first:?}; one move {second:?}; every shape moved {every:?}");
    assert!(first <= plain * 6, "the first read costs {first:?} against the yardstick's {plain:?}");
    assert!(second * 20 <= first, "a put right after one move costs {second:?} against the first read's {first:?}");
    assert!(every <= first * 3, "an edit of every shape costs {every:?} against the first read's {first:?}");
}

#[test]
#[ignore = "a timing: run by hand beside the yardstick"]
fn a_page_of_30000_shapes_costs_no_more_than_the_cap_and_the_compares() {
    let ten = boxes(10_000);
    let thirty = boxes(30_000);
    let (_, base) = timed(|| Scene::read(&ten, 1, &mut plenty()));
    let (mut scene, first) = timed(|| Scene::read(&thirty, 1, &mut plenty()));
    let mut next = thirty.clone();
    shift(&mut next, 77, 0.0, 0.25);
    let (_, again) = timed(|| scene.put_right(&next, 1, &mut plenty()));
    eprintln!("TIMED 10,000 shapes {base:?}; 30,000 shapes {first:?}; one move after it {again:?}");
    assert!(first <= base * 3, "30,000 shapes cost {first:?} against the 10,000's {base:?}");
    assert!(again * 10 <= first, "an edit after the first read costs {again:?} against {first:?}");
}

/// The timed twin of `a_wide_group_is_not_read_past_the_work_the_put_right_has_left`: a put right with one step left for the members of a group of 10,000 reads one of them, and costs a small
/// part of the read of the whole group (it used to read up to the cap of 10,000 members before the work was asked).
#[test]
#[ignore = "a timing: run by hand beside the whole read"]
fn a_put_right_with_one_step_left_for_a_wide_group_costs_a_fraction_of_reading_it() {
    let members: Vec<_> = (1..=10_000u32).map(|k| super::build::placeable(100_000 + k, (f64::from(k % 100), f64::from(k / 100), 0.5, 0.5))).collect();
    let doc = page(vec![super::build::group(1, (0.0, 0.0, 100.0, 100.0), "8", members)], Vec::new());
    let (_, whole) = timed(|| Scene::read(&doc, 1, &mut plenty()));
    let (stopped, short) = timed(|| {
        let mut scene = Scene::new(1);
        scene.put_right(&doc, 1, &mut diacraft_route::Work::new(3)).incomplete
    });
    eprintln!("TIMED a group of 10,000 members read whole {whole:?}; with three steps of work {short:?}");
    assert!(stopped, "three steps are not the group");
    assert!(short * 50 <= whole, "a put right with three steps costs {short:?} against {whole:?} for the group read whole");
}

#[test]
#[ignore = "a timing: run by hand beside the yardstick"]
fn a_page_of_10000_shapes_and_10000_connectors_and_deep_groups_beside_the_yardstick() {
    let mut doc = super::counted::big();
    let plain = yardstick(&doc);
    let (mut scene, first) = timed(|| Scene::read(&doc, 1, &mut plenty()));
    shift(&mut doc, 4_321, 0.0, 0.25);
    let (_, second) = timed(|| scene.put_right(&doc, 1, &mut plenty()));
    for at in 0..20_000 {
        nudge_at(&mut doc, at, 0.25);
    }
    let (_, every) = timed(|| scene.put_right(&doc, 1, &mut plenty()));
    // 150 nested groups, and one of 20,000 members.
    let mut inner = super::build::placeable(1_000, (0.0, 0.0, 1.0, 1.0));
    for level in 0..150u32 {
        inner = super::build::group(900 - level, (0.0, 0.0, 4.0, 4.0), "8", vec![inner]);
    }
    let deep = page(vec![inner], Vec::new());
    let members: Vec<_> = (1..=20_000u32).map(|k| super::build::placeable(100_000 + k, (f64::from(k % 100), f64::from(k / 100), 0.5, 0.5))).collect();
    let wide = page(vec![super::build::group(1, (0.0, 0.0, 100.0, 200.0), "8", members)], Vec::new());
    let (_, deep_read) = timed(|| Scene::read(&deep, 1, &mut plenty()));
    let (_, wide_read) = timed(|| Scene::read(&wide, 1, &mut plenty()));
    eprintln!(
        "TIMED yardstick {plain:?}; first read {first:?}; one move {second:?}; every shape moved {every:?}; 150 deep groups {deep_read:?}; a group of 20,000 members {wide_read:?}"
    );
    assert!(first <= plain * 8, "the first read costs {first:?} against the yardstick's {plain:?}");
    assert!(second * 10 <= first, "one move costs {second:?} against the first read's {first:?}");
    assert!(every <= first * 3, "an edit of every shape costs {every:?} against the first read's {first:?}");
    assert!(deep_read * 100 <= first, "150 nested groups cost {deep_read:?}");
    assert!(wide_read <= first, "a group of 20,000 members costs {wide_read:?} against {first:?}");
}
