//! What the held values cost (the crafted inputs of the plan): **65,536 named definitions in each family**, **shapes that each hold all eight groups**, and **a style
//! attribute of one MiB that every group of every shape inherits**. Each has a counted cost test (the work is linear, and the figures are exact), and a timed run beside a yardstick
//! of plain content of the same size (`--ignored`, printed, never asserted: a test asserts no time). The two that must hold in a child process, under `ulimit -v 3000000`, are
//! **refused and not aborted** (a million shapes) and **read within the address space** (the definitions and the one-MiB attribute).

use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{DefFamily, HeldGroup, MAX_DEFINITIONS};
use diacraft_odg::{Limits, read, read_partial};

use crate::drawing::Drawing;

const SCENARIO: &str = "ODFTAG2_COST_SCENARIO";

/// The six families of definitions, each with the element that makes one and the attributes that name it.
const FAMILIES: [(&str, DefFamily); 6] = [
    ("marker", DefFamily::Marker),
    ("gradient", DefFamily::Gradient),
    ("opacity", DefFamily::Opacity),
    ("hatch", DefFamily::Hatch),
    ("stroke-dash", DefFamily::StrokeDash),
    ("fill-image", DefFamily::FillImage),
];

/// `n` definitions of each family, named `<family><k>` with the digits padded so that every name is as long as every other.
fn definitions(n: usize) -> String {
    let mut out = String::new();
    for (word, _) in FAMILIES {
        for k in 0..n {
            out.push_str(&format!(r##"<draw:{word} draw:name="{word}{k:06}" draw:display-name="d" draw:style="s"/>"##));
        }
    }
    out
}

fn items_of(common: &str) -> u64 {
    read(&Drawing::a4().common(common).page("").bytes(), &Limits::default()).expect("reads").1.work.items
}

/// **65,536 definitions in each family** are held, exactly, in file order, and the cost of reading them is **linear**: the items of `4n` of each family less those of `n` are three
/// times those of `n` less none, and the document holds `6 * 65,536` of them at the most. One past the cap of a family is refused before it is made.
#[test]
fn sixty_five_thousand_definitions_in_each_family_are_held_at_a_cost_that_is_linear() {
    let (none, one, four) = (items_of(""), items_of(&definitions(1_000)), items_of(&definitions(4_000)));
    assert_eq!(four - none, 4 * (one - none), "linear: every definition costs what every other does");
    // The cap: 65,536 of each family at once.
    let (doc, report) = Drawing::a4().common(&definitions(MAX_DEFINITIONS)).page("").read();
    let odf = doc.ext.get().expect("the document's slot").odf.clone();
    for (_, family) in FAMILIES {
        assert_eq!(odf.definitions.iter().filter(|d| d.family == family).count(), MAX_DEFINITIONS, "{family:?}");
    }
    assert_eq!(odf.definitions.len(), 6 * MAX_DEFINITIONS);
    // Each is as written, in file order: the first of the family, then the next, and the names that follow each other.
    assert_eq!(&*odf.definitions[0].name, "marker000000");
    assert_eq!(&*odf.definitions[MAX_DEFINITIONS - 1].name, format!("marker{:06}", MAX_DEFINITIONS - 1));
    assert_eq!(&*odf.definitions[MAX_DEFINITIONS].name, "gradient000000");
    // The read's cost is the items the figure says: no more than the cap, and the work is counted.
    assert!(report.work.items < 20_000_000, "{}", report.work.items);
    drop(doc);
    // One past the cap of one family is refused, in words that name the family.
    let over = format!(r##"{}<draw:hatch draw:name="one more" draw:style="s"/>"##, definitions(MAX_DEFINITIONS));
    let (result, _) = read_partial(&Drawing::a4().common(&over).page("").bytes(), &Limits::default());
    let words = result.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(words.contains("hatch definitions"), "{words}");
}

/// A style that gives a shape one value of every group R22 lists (the fill, the dash, the line ends, the shadow, the text, the frame's layer, the glue and the geometry): the
/// groups are the eight of `HeldGroup`.
const ALL_GROUPS_STYLE: &str = concat!(
    r##"<style:style style:name="a" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H" draw:opacity-name="O" "##,
    r##"draw:stroke="dash" draw:stroke-dash="D" draw:stroke-linejoin="round" draw:marker-start="Arrow" draw:marker-start-width="0.3cm" "##,
    r##"draw:shadow="visible" draw:shadow-blur="0.1cm" draw:textarea-vertical-align="justify" fo:padding="0.1cm" style:writing-mode="tb-rl"/></style:style>"##,
);

const NAMED: &str = concat!(
    r##"<draw:marker draw:name="Arrow" svg:viewBox="0 0 1 1" svg:d="M0 0"/><draw:hatch draw:name="H" draw:style="single"/><draw:opacity draw:name="O" draw:style="linear"/>"##,
    r##"<draw:stroke-dash draw:name="D" draw:style="rect"/>"##,
);

/// A rectangle on the style `a` that holds a value of every group: a turned frame, a layer, a glue point and a corner radius.
fn all_groups(k: usize) -> String {
    format!(
        r##"<draw:rect draw:style-name="a" draw:layer="layout" draw:corner-radius="0.1cm" draw:transform="rotate (0.5) translate ({}mm 4cm)" svg:width="1cm" svg:height="1cm"><draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm" draw:escape-direction="auto"/></draw:rect>"##,
        k % 10
    )
}

fn shapes(n: usize) -> Drawing {
    Drawing::a4().common(NAMED).auto(ALL_GROUPS_STYLE).page(&(0..n).map(all_groups).collect::<String>())
}

/// **Shapes that each hold all eight groups**: each holds a value of every group, the cost is the same for each (counted, exactly linear), and the values of a style are shared by
/// the shapes that name it (one list of attributes behind every pointer).
#[test]
fn shapes_that_each_hold_all_the_groups_cost_the_same_each_and_share_their_style_s_values() {
    let items = |n: usize| read(&shapes(n).bytes(), &Limits::default()).expect("reads").1.work.items;
    // A shape's own `draw:transform` is a list of its own, shared by the shapes that write the same one (ten of them here, all made by the hundredth shape): after those, every
    // shape costs what every other does.
    let (a, b, c) = (items(100), items(1_100), items(2_100));
    assert_eq!(c - b, b - a, "linear: every shape costs what every other does");
    let (doc, _) = shapes(3).read();
    let page = doc.pages.get(0).unwrap();
    let held = |k: usize| page.shapes.get(k).unwrap().ext.get().map(|d| d.odf.held.clone()).unwrap_or_default();
    let groups: std::collections::HashSet<HeldGroup> = held(0).iter().map(|h| h.group).collect();
    for group in [HeldGroup::Fill, HeldGroup::Line, HeldGroup::LineEnds, HeldGroup::Shadow, HeldGroup::Text, HeldGroup::Frame, HeldGroup::Geometry] {
        assert!(groups.contains(&group), "{group:?} in {groups:?}");
    }
    // The glue point is on its row (the eighth group).
    let row = page
        .shapes
        .get(0)
        .unwrap()
        .sheet
        .section(&diacraft_doc::SectionKey::new("Connection", None))
        .and_then(|s| s.row(&diacraft_doc::RowKey::Ix(4)))
        .cloned();
    assert!(row.is_some_and(|r| r.ext.get().is_some_and(|d| d.odf.held.as_ref().is_some_and(|h| h.group == HeldGroup::Glue))));
    // The attributes of the style's values are one list for the three shapes: the pointer is the same.
    for at in 0..held(0).len() {
        let (a, b) = (held(0), held(1));
        if let (Some(x), Some(y)) = (a.get(at).and_then(|h| h.attrs.shared_list()), b.get(at).and_then(|h| h.attrs.shared_list())) {
            // A value of the shape's own element (its transform) is its own; the style's are shared.
            if a[at].attrs.get("draw:transform").is_none() {
                assert!(Arc::ptr_eq(x, y), "value {at} is one list for every shape that names the style");
            }
        }
    }
}

/// **A style attribute of one MiB that every group of every shape inherits**: the style holds a megabyte for each of its values (the hatch's name, the dash, the marker, the
/// shadow, the padding), and `n` shapes name it. The values are shared, so the items of `2n` shapes are those of `n` and `n` times what a shape costs, **whatever the size of the
/// attribute**, and the document holds each megabyte once.
#[test]
fn a_one_mebibyte_attribute_of_a_style_is_shared_by_every_group_of_every_shape() {
    let mib = "x".repeat(1 << 20);
    let style = |size: &str| {
        format!(
            r##"<style:style style:name="a" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="{size}" draw:stroke-dash="{size}" draw:marker-start="{size}" draw:shadow-blur="{size}" fo:padding="{size}" draw:opacity-name="{size}"/></style:style>"##
        )
    };
    let items = |size: &str, n: usize| {
        let body: String =
            (0..n).map(|k| format!(r##"<draw:rect draw:style-name="a" svg:x="{}mm" svg:width="1cm" svg:height="1cm"/>"##, k % 1_000)).collect();
        read(&Drawing::a4().auto(&style(size)).page(&body).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    // The size of the attribute only costs once, with the style: one item for 64 bytes, six times (and the style's own copy).
    let (small_n, small_2n) = (items("x", 500), items("x", 1_000));
    let (big_n, big_2n) = (items(&mib, 500), items(&mib, 1_000));
    assert_eq!(big_2n - big_n, small_2n - small_n, "500 more shapes cost the same whatever the size of the attribute they inherit");
    assert!(big_n > small_n + 6 * (1 << 20) / 64, "and the megabytes are charged, with the style that holds them: {big_n} {small_n}");
    // The shapes hold the megabytes by one pointer: the shapes' values are one list each, and the document's text is the megabytes once (the style's), not once for each shape.
    let body: String = (0..300).map(|k| format!(r##"<draw:rect draw:style-name="a" svg:x="{k}mm" svg:width="1cm" svg:height="1cm"/>"##)).collect();
    let (doc, _) = Drawing::a4().auto(&style(&mib)).page(&body).read();
    let page = doc.pages.get(0).unwrap();
    let lists: Vec<_> = page
        .shapes
        .iter()
        .filter_map(|s| s.ext.get().and_then(|d| d.odf.held.iter().find(|h| h.attrs.get("draw:fill").is_some()).cloned()))
        .filter_map(|h| h.attrs.shared_list().cloned())
        .collect();
    assert_eq!(lists.len(), 300);
    assert!(lists.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])), "the 300 shapes hold one list behind their pointers");
}

fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact held_cost::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// The scenarios of the child processes, each under `ulimit -v 3000000`: they end in an error or a document, never an abort.
#[test]
#[ignore = "run by the tests of this file in a child process"]
fn child() {
    let scenario = std::env::var(SCENARIO).unwrap_or_default();
    match scenario.as_str() {
        // A million shapes that each hold every group: the file is refused (the tree's cap, or the items'), in words, and the process holds a bounded part of its address space.
        "a_million_shapes" => {
            let bytes = shapes(1_000_000).bytes();
            let (result, _) = read_partial(&bytes, &Limits::default());
            let class = result.err().map(|e| e.class()).unwrap_or("read");
            assert!(matches!(class, "too_many_items" | "xml_too_complex" | "too_much_xml"), "{class}");
            println!("\nRESULT a_million_shapes class={class} VmHWM={}KiB", status_kib("VmHWM"));
        }
        // The definitions at the cap, in every family, and the one-MiB attribute with 20,000 shapes that inherit it: read within the address space.
        "definitions_at_the_cap" => {
            let (doc, report) = Drawing::a4().common(&definitions(MAX_DEFINITIONS)).page("").read();
            assert_eq!(doc.ext.get().map(|d| d.odf.definitions.len()), Some(6 * MAX_DEFINITIONS));
            println!("\nRESULT definitions_at_the_cap items={} VmHWM={}KiB", report.work.items, status_kib("VmHWM"));
        }
        "one_mebibyte_attribute" => {
            let mib = "x".repeat(1 << 20);
            let style = format!(
                r##"<style:style style:name="a" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="{mib}" draw:stroke-dash="{mib}" draw:marker-start="{mib}" draw:shadow-blur="{mib}" fo:padding="{mib}" draw:opacity-name="{mib}"/></style:style>"##
            );
            let body: String = (0..20_000)
                .map(|k| format!(r##"<draw:rect draw:style-name="a" svg:x="{}mm" svg:width="1cm" svg:height="1cm"/>"##, k % 1_000))
                .collect();
            let (doc, report) = Drawing::a4().auto(&style).page(&body).read();
            assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(20_000));
            println!("\nRESULT one_mebibyte_attribute items={} VmHWM={}KiB", report.work.items, status_kib("VmHWM"));
        }
        // A text of a million paragraphs, each with a span, in one shape: the paragraphs are charged as they are read.
        "a_million_paragraphs" => {
            let body = r##"<text:p text:style-name="P1">a<text:span text:style-name="T1">b</text:span></text:p>"##.repeat(1_000_000);
            let shape = format!(
                r##"<draw:custom-shape svg:width="1cm" svg:height="1cm">{body}<draw:enhanced-geometry svg:viewBox="0 0 1 1"/></draw:custom-shape>"##
            );
            let styles = r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="start"/></style:style><style:style style:name="T1" style:family="text"><style:text-properties fo:text-transform="lowercase"/></style:style>"##;
            let (result, report) = read_partial(&Drawing::a4().auto(styles).page(&shape).bytes(), &Limits::default());
            let class = result.err().map(|e| e.class()).unwrap_or("read");
            println!("\nRESULT a_million_paragraphs class={class} items={} VmHWM={}KiB", report.work.items, status_kib("VmHWM"));
        }
        other => panic!("no scenario {other}"),
    }
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_million_shapes_that_each_hold_all_the_groups_are_refused_and_do_not_abort() {
    let (ended_well, stdout) = run_child("a_million_shapes", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT a_million_shapes class="), "{stdout}");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_text_of_a_million_paragraphs_is_read_or_refused_within_the_address_space() {
    let (ended_well, stdout) = run_child("a_million_paragraphs", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT a_million_paragraphs class="), "{stdout}");
}

/// The paragraphs of a text are charged as they are read: the items of `2n` paragraphs are those of `n` and `n` times what a paragraph with a span costs (counted).
#[test]
fn the_paragraphs_of_a_text_cost_the_same_each() {
    let styles = r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="start"/></style:style><style:style style:name="T1" style:family="text"><style:text-properties fo:text-transform="lowercase"/></style:style>"##;
    let items = |n: usize| {
        let body = r##"<text:p text:style-name="P1">a<text:span text:style-name="T1">b</text:span></text:p>"##.repeat(n);
        let shape = format!(
            r##"<draw:custom-shape svg:width="1cm" svg:height="1cm">{body}<draw:enhanced-geometry svg:viewBox="0 0 1 1"/></draw:custom-shape>"##
        );
        read(&Drawing::a4().auto(styles).page(&shape).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    let (a, b, c) = (items(10), items(1_010), items(2_010));
    assert_eq!(c - b, b - a, "every paragraph costs what every other does");
    assert!(b - a >= 1_000 * 3, "and a paragraph and its span are charged: {}", b - a);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn the_definitions_of_every_family_at_the_cap_are_read_within_the_address_space() {
    let (ended_well, stdout) = run_child("definitions_at_the_cap", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_one_mebibyte_attribute_inherited_by_twenty_thousand_shapes_is_read_within_the_address_space() {
    let (ended_well, stdout) = run_child("one_mebibyte_attribute", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// By hand, not part of the suite (a measure, **a test asserts no time**): each of the three inputs read beside a yardstick of plain content of the same size (the same number of
/// elements and bytes that the reader counts and does not keep: filler among the master styles for the definitions, plain rectangles for the shapes). Run with
/// `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand"]
fn timed_beside_a_yardstick() {
    let time = |what: &str, bytes: &[u8]| {
        let start = std::time::Instant::now();
        let (result, report) = read_partial(bytes, &Limits::default());
        println!(
            "TIMED {what}: {:?} ({}), items {}",
            start.elapsed(),
            result.err().map_or("read".to_owned(), |e| e.class().to_owned()),
            report.work.items
        );
    };
    // 65,536 definitions in each family, against the same number of elements of an unknown vocabulary among the master styles (counted, not kept).
    let mut yard = Drawing::a4();
    yard.ns.push_str(r#" xmlns:x="urn:example:filler""#);
    yard.masters.push_str(&definitions(MAX_DEFINITIONS).replace("draw:", "x:"));
    time("definitions (6 x 65,536)", &Drawing::a4().common(&definitions(MAX_DEFINITIONS)).page("").bytes());
    time("yardstick of the same elements", &yard.page("").bytes());
    // 150,000 shapes that each hold all the groups, against the same number of plain rectangles.
    time("150,000 shapes of all the groups", &shapes(150_000).bytes());
    let plain: String = (0..150_000).map(|k| format!(r##"<draw:rect svg:x="{}mm" svg:width="1cm" svg:height="1cm"/>"##, k % 1_000)).collect();
    time("150,000 plain rectangles", &Drawing::a4().page(&plain).bytes());
}
