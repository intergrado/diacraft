//! Crafted inputs whose cost a fuzzer would not find (plan, and the lessons of earlier readers: chains, deep nesting, many
//! siblings, a huge page), each held to a count of the units the reader walks and never to a time. A cost claim holds for the shape a test
//! builds: these build the shapes each loop walks (nesting, attributes, pages, shapes, chains of styles, spans, path commands) and say which
//! loop they hold. The memory cases at the end are ignored and run by hand, one per process, under `ulimit -v` (see `limits.rs`).

use diacraft_odg::{Limits, read, read_partial};

use crate::drawing::{Drawing, as_text, cell, odf, only, page, shape};

fn rects(n: usize) -> String {
    let mut out = String::with_capacity(n * 80);
    for k in 0..n {
        out.push_str(&format!(r#"<draw:rect svg:x="{}mm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#, k % 200));
    }
    out
}

#[test]
fn a_hundred_thousand_nested_groups_are_refused_by_the_depth_cap_at_once() {
    let depth = 100_000;
    let shapes = format!("{}{}", "<draw:g>".repeat(depth), "</draw:g>".repeat(depth));
    let (result, report) = read_partial(&Drawing::a4().page(&shapes).bytes(), &Limits::default());
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    // The parser stops at the cap (200 levels) for content.xml, after a few hundred events and not a hundred thousand.
    assert!(report.work.package.xml_events < 2_000, "{}", report.work.package.xml_events);
}

#[test]
fn nested_groups_under_the_cap_are_numbered_and_kept_in_steps_linear_in_their_count() {
    let run = |depth: usize| {
        let shapes = format!(
            "{}<draw:rect svg:width=\"1cm\" svg:height=\"1cm\"/>{}<draw:rect svg:width=\"1cm\" svg:height=\"1cm\"/>",
            "<draw:g>".repeat(depth),
            "</draw:g>".repeat(depth)
        );
        let (doc, report) = Drawing::a4().page(&shapes).read();
        // The groups are modelled, one inside the other, and the rectangle inside the innermost; the rectangle after is the page's second.
        assert!(report.index.page(0).unwrap().kept().is_empty());
        assert_eq!(report.index.page(0).unwrap().modelled(), depth + 2);
        assert_eq!(shape(&doc, 0, 1).id, u32::try_from(depth + 2).unwrap());
        let mut at = only(&doc);
        for _ in 1..depth {
            at = at.children.get(0).expect("a group inside a group");
        }
        assert_eq!(at.children.len(), 1);
        report.work.elements
    };
    let (small, large) = (run(19), run(190));
    eprintln!("nested groups: elements walked {small} at 19 deep, {large} at 190");
    assert!(large < small * 12, "{small} then {large}");
}

#[test]
fn a_million_attributes_on_one_element_cost_a_million_steps() {
    let run = |n: usize| {
        let mut attrs = String::with_capacity(n * 16);
        for k in 0..n {
            attrs.push_str(&format!(r#" x:a{k}="1""#));
        }
        let shape = format!(r#"<draw:rect xmlns:x="urn:example:many" svg:width="1cm" svg:height="1cm"{attrs}/>"#);
        let (doc, report) = Drawing::a4().page(&shape).read();
        assert_eq!(odf(only(&doc)).raw.len(), n + 1, "every one kept, and the declaration of their prefix beside them");
        assert_eq!(report.kept("attribute"), n as u64);
        (report.work.attributes, report.work.items)
    };
    let (attributes, items) = run(1_000_000);
    assert!((1_000_000..1_002_000).contains(&attributes), "{attributes}");
    // Two items for each kept attribute (its pair of strings), and one for each 64 bytes of their names and values, as text at nine eighths.
    let bytes: u64 = as_text((0..1_000_000u64).map(|k| format!("x:a{k}").len() as u64 + 1).sum::<u64>()).div_ceil(64);
    assert!((2_000_000 + bytes..2_002_000 + bytes).contains(&items), "{items} against {}", 2_000_000 + bytes);
    let (small, _) = run(100_000);
    eprintln!("attributes: {small} resolved for 100,000, {attributes} for 1,000,000, items {items}");
    assert!(attributes < small * 11, "{small} then {attributes}");
}

#[test]
fn fifty_pages_with_thirty_five_thousand_shapes_on_one_cost_what_they_hold() {
    let run = |big: usize| {
        let mut d = Drawing::a4().page(&rects(big));
        for _ in 1..50 {
            d = d.page(&rects(1));
        }
        let (doc, report) = d.read();
        assert_eq!(doc.pages.len(), 50);
        assert_eq!(page(&doc, 0).shapes.len(), big);
        assert_eq!(report.stats.max_shapes_in_a_page, big);
        assert_eq!(report.index.page(0).unwrap().modelled(), big);
        assert_eq!(report.index.high_water(0), Some(u32::try_from(big).unwrap()));
        (report.work.elements, report.work.attributes, report.work.style_steps, report.stats.cells)
    };
    let large = run(35_000);
    let small = run(3_500);
    eprintln!("fifty pages (elements, attributes, style steps, cells): {small:?} at 3,500 shapes, {large:?} at 35,000");
    for (name, s, l) in [
        ("elements", small.0, large.0),
        ("attributes", small.1, large.1),
        ("style steps", small.2, large.2),
        ("cells", small.3 as u64, large.3 as u64),
    ] {
        assert!(l <= s * 11, "{name}: {s} then {l}");
    }
}

#[test]
fn a_long_chain_of_styles_is_resolved_in_steps_linear_in_its_length() {
    let run = |n: usize| {
        let mut d = Drawing::a4();
        let mut common = String::new();
        for k in 0..n {
            let parent = if k == 0 { String::new() } else { format!(r#" style:parent-style-name="s{}""#, k - 1) };
            common.push_str(&format!(r##"<style:style style:name="s{k}" style:family="graphic"{parent}><style:graphic-properties svg:stroke-width="{k}mm"/></style:style>"##));
        }
        d.common = common;
        // Every shape names the deepest style through its own automatic style.
        d.auto = format!(
            r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="s{}"><style:graphic-properties draw:fill-color="#123456"/></style:style>"##,
            n - 1
        );
        let shapes: String = (0..100).map(|_| r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"#).collect();
        let (doc, report) = d.page(&shapes).read();
        assert_eq!(doc.styles.len(), n + 1);
        assert_eq!(crate::drawing::text(cell(only(&doc), "FillForegnd")), "#123456");
        report.work.style_steps
    };
    let (small, large) = (run(1_000), run(10_000));
    eprintln!("style chain: {small} style steps at 1,000 styles, {large} at 10,000");
    assert!(large < small * 12, "{small} then {large}");
}

#[test]
fn a_long_chain_of_automatic_parents_shared_by_many_shapes_is_walked_once() {
    let run = |shapes: usize| {
        let mut auto = String::new();
        for k in 0..64 {
            let parent = if k == 0 { String::new() } else { format!(r#" style:parent-style-name="a{}""#, k - 1) };
            auto.push_str(&format!(
                r#"<style:style style:name="a{k}" style:family="graphic"{parent}><style:graphic-properties svg:stroke-width="{k}mm"/></style:style>"#
            ));
        }
        let mut body = String::new();
        for k in 0..shapes {
            auto.push_str(&format!(r##"<style:style style:name="gr{k}" style:family="graphic" style:parent-style-name="a63"><style:graphic-properties draw:fill-color="#{:06x}"/></style:style>"##, k % 0xFFFFFF));
            body.push_str(&format!(r#"<draw:rect draw:style-name="gr{k}" svg:width="1cm" svg:height="1cm"/>"#));
        }
        let (_, report) = Drawing::a4().auto(&auto).page(&body).read();
        report.work.style_steps
    };
    let (small, large) = (run(200), run(2_000));
    eprintln!("automatic parents: {small} style steps for 200 shapes, {large} for 2,000");
    assert!(large < small * 12, "{small} then {large}");
}

#[test]
fn many_spans_and_spaces_in_one_paragraph_are_walked_once_each() {
    let run = |n: usize| {
        let mut p = String::from("<text:p>");
        for _ in 0..n {
            p.push_str(r#"<text:span text:style-name="T1">a</text:span>   <text:s text:c="3"/>"#);
        }
        p.push_str("</text:p>");
        let (doc, report) = Drawing::a4()
            .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
            .page(&format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{p}</draw:rect>"#))
            .read();
        assert!(only(&doc).text.is_some());
        report.work.text_units
    };
    let (small, large) = (run(1_000), run(10_000));
    eprintln!("spans: {small} text units for 1,000 spans, {large} for 10,000");
    assert!(large < small * 11, "{small} then {large}");
}

#[test]
fn a_space_count_is_charged_before_its_spaces_are_made() {
    let p = format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"#, r#"<text:s text:c="65536"/>"#.repeat(2_000));
    let limits = Limits { max_items: 100_000, ..Limits::default() };
    let err = read(&Drawing::a4().page(&p).bytes(), &limits).err().map(|e| e.class());
    assert_eq!(err, Some("too_many_items"));
}

#[test]
fn path_commands_past_a_page_s_budget_are_too_complex() {
    let d = format!("M 0 0{}", " L 1 1".repeat(1_000_001));
    let shape = format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="{d}"/>"#);
    let err = read(&Drawing::a4().page(&shape).bytes(), &Limits::default()).err().map(|e| e.class());
    assert_eq!(err, Some("xml_too_complex"));
    let points = "1,1 ".repeat(1_000_001);
    let shape = format!(r#"<draw:polygon svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" draw:points="{points}"/>"#);
    let err = read(&Drawing::a4().page(&shape).bytes(), &Limits::default()).err().map(|e| e.class());
    assert_eq!(err, Some("xml_too_complex"));
    // The budget is a page's: two pages of 600,000 each read (ten paths of 60,000 commands on each), given the items their rows cost.
    let tenth = format!("M 0 0{}", " L 1 1".repeat(59_999));
    let shape = format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="{tenth}"/>"#).repeat(10);
    let limits = Limits { max_items: 100_000_000, ..Limits::default() };
    let (doc, report) = read(&Drawing::a4().page(&shape).page(&shape).bytes(), &limits).unwrap();
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(report.work.path_units, 1_200_000);
}

#[test]
fn the_caps_of_the_vsdx_reader_hold() {
    let d = Drawing::a4().page(&rects(10));
    for (limits, class) in [
        (Limits { max_shapes: 5, ..Limits::default() }, "too_many_shapes"),
        (Limits { max_shapes_per_page: 5, ..Limits::default() }, "too_many_shapes_in_a_page"),
        (Limits { max_cells: 50, ..Limits::default() }, "too_many_cells"),
        (Limits { max_items: 50, ..Limits::default() }, "too_many_items"),
    ] {
        assert_eq!(read(&d.bytes(), &limits).err().map(|e| e.class()), Some(class));
    }
    let pages = (0..6).fold(Drawing::a4(), |d, _| d.page(""));
    assert_eq!(read(&pages.bytes(), &Limits { max_pages: 5, ..Limits::default() }).err().map(|e| e.class()), Some("too_many_pages"));
    let styles: String = (0..6).map(|k| format!(r#"<style:style style:name="s{k}" style:family="graphic"/>"#)).collect();
    let d = Drawing::a4().common(&styles).page("");
    assert_eq!(read(&d.bytes(), &Limits { max_styles: 5, ..Limits::default() }).err().map(|e| e.class()), Some("too_many_styles"));
}

/// The peak resident size of this process so far, in kilobytes (Linux `VmHWM`), or 0 where it cannot be read.
fn peak_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status.lines().find_map(|l| l.strip_prefix("VmHWM:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()).unwrap_or(0)
}

/// Reads `bytes` with the peak reset first (Linux: writing 5 to `/proc/self/clear_refs`), so that what is printed is the read's own peak
/// with `bytes` held, not the building of them. Gives the error class of a refused file.
fn measure(name: &str, bytes: &[u8]) -> Option<&'static str> {
    measured(name, bytes).0
}

/// [`measure`], and the peak of the read in kilobytes.
fn measured(name: &str, bytes: &[u8]) -> (Option<&'static str>, u64) {
    let before = peak_kb();
    let _reset = std::fs::write("/proc/self/clear_refs", "5");
    let (result, report) = read_partial(bytes, &Limits::default());
    let class = result.as_ref().err().map(|e| e.class());
    eprintln!(
        "{name}: file {} bytes, peak before the read {before} kB, peak during it {} kB, items {}, text bytes {}, media bytes {}, result {}",
        bytes.len(),
        peak_kb(),
        report.work.items,
        report.work.text_bytes,
        report.work.media_bytes,
        class.unwrap_or("read")
    );
    drop(result);
    (class, peak_kb())
}

/// A package of `d`'s parts and of `extra` parts, each made by `make` when it is written, so that a test holds one large part at a time.
/// Every part is deflated, so that the file stays small however large its parts inflate (as a hostile file's do).
pub(crate) fn part_by_part(d: Drawing, extra: &[&str], make: &dyn Fn(&str) -> Vec<u8>) -> Vec<u8> {
    use diacraft_container::zip::{ArchiveWriter, Method};
    let files = d.files();
    drop(d);
    let mut rows: Vec<(String, &str)> = files.iter().map(|(n, _)| (n.clone(), "text/xml")).collect();
    rows.extend(extra.iter().map(|n| ((*n).to_owned(), if n.ends_with(".png") { "image/png" } else { "application/octet-stream" })));
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    let manifest = crate::build::manifest(crate::build::DRAWING, &rows);
    let mut w = ArchiveWriter::new();
    w.add("mimetype", crate::build::DRAWING.as_bytes(), Method::Stored).unwrap();
    for (name, data) in files {
        w.add(&name, &data, Method::Deflated).unwrap();
    }
    for name in extra {
        let data = make(name);
        w.add(name, &data, Method::Deflated).unwrap();
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// A PNG's signature and then zeros: a picture of `len` bytes. (**Not** bytes for parts nothing names: a read inflates only the parts the drawing names so
/// such parts cost a read and a save nothing, and a case that fills its package with them measures less than it says; the every-budget cases hold their package's
/// bytes as pictures that frames show.)
pub(crate) fn picture(len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    out[..4].copy_from_slice(&[0x89, b'P', b'N', b'G']);
    out
}

/// Rectangles with text, each of its own size, so that no section is shared.
pub(crate) fn unshared_rects(n: usize) -> String {
    (0..n)
        .map(|k| {
            format!(
                r#"<draw:rect svg:x="{}mm" svg:y="1cm" svg:width="{}.{:06}mm" svg:height="1cm"><text:p>t</text:p></draw:rect>"#,
                k % 200,
                10 + k / 1_000_000,
                k % 1_000_000
            )
        })
        .collect()
}

/// Filler elements of a vocabulary the reader does not know.
pub(crate) fn filler(n: usize) -> String {
    (0..n).map(|k| format!(r#"<x:i n="{k}"/>"#)).collect()
}

/// Two trees at the container's item cap: what `Package::read` holds before the reader starts (run by hand: `limits.rs`'s table).
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_two_trees_at_the_item_cap() {
    // Each filler element is two items, itself and its attribute.
    let n = diacraft_odg::limits::MAX_READ_XML_NODES / 2 - 10_000;
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:filler""#);
    d.common = filler(n);
    d.pages = format!(r#"<draw:page draw:name="p" draw:master-page-name="Default"/>{}"#, filler(n));
    let bytes = d.bytes();
    drop(d);
    measure("two trees", &bytes);
}

/// The most shapes one page may keep unmodelled, each a kept element: what the model holds of markup it does not model.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_a_page_of_kept_shapes_at_the_cap() {
    // Four items each in the tree (the element and its three attributes), up to the reader's tree cap.
    let n = diacraft_odg::limits::MAX_READ_XML_NODES / 4 - 20_000;
    // Hyperlink wrappers are the one kind of shape element still kept whole (custom shapes are modelled).
    let shapes: String = (0..n).map(|k| format!(r#"<draw:a draw:name="c{k}" svg:width="1cm" svg:height="1cm"/>"#)).collect();
    let bytes = Drawing::a4().page(&shapes).bytes();
    drop(shapes);
    measure("kept shapes", &bytes);
}

/// Modelled shapes up to the item cap, each a rectangle with text and a size of its own, so that no section is shared: what the model
/// holds of what it models, at its worst.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_a_page_of_modelled_shapes() {
    let shapes = unshared_rects(150_000);
    let bytes = Drawing::a4().page(&shapes).bytes();
    drop(shapes);
    measure("modelled shapes", &bytes);
}

/// The review's spaces: one rectangle of 19,000 `text:s` of 65,536, a 2,477-byte file that once made 1.2 GB of text; and the text cap's
/// worth, 1,024 of them.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_spaces() {
    for (name, count) in [("spaces past the text cap", 19_000), ("spaces at the text cap", 1_024)] {
        let p =
            format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"#, r#"<text:s text:c="65536"/>"#.repeat(count));
        measure(name, &Drawing::a4().page(&p).bytes());
    }
}

/// The review's polygons: six concave regular polygons of 159,999 corners on one page, 1,325 bytes; and one on each of six pages.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_concave_polygons() {
    let polygon = r#"<draw:regular-polygon svg:width="1cm" svg:height="1cm" draw:corners="159999" draw:concave="true" draw:sharpness="50%"/>"#;
    measure("six polygons on a page", &Drawing::a4().page(&polygon.repeat(6)).bytes());
    let pages = (0..6).fold(Drawing::a4(), |d, _| d.page(polygon));
    measure("six polygons on six pages", &pages.bytes());
}

/// One 256 MiB picture part named by 10,000 frames: one copy of it, charged once.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_a_picture_named_by_ten_thousand_frames() {
    let frame = r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/big.png"/></draw:frame>"#;
    let bytes = part_by_part(Drawing::a4().page(&frame.repeat(10_000)), &["Pictures/big.png"], &|_| picture(256 * 1024 * 1024));
    measure("one picture, 10,000 frames", &bytes);
}

/// The hostile sum, every budget at once (the review's 5.59 GB combination): the package at its 1 GiB cap, a 250 MB picture shown by four
/// frames and three of 240 MB shown by one frame each (970 MB of media; parts nothing names are not inflated, so they would be no cost: see
/// `every_budget`); a `content.xml` near the reader's tree cap whose two concave polygons (the model's costliest items, 54 bytes each) and
/// unshared rectangles take the items near their cap, with the text at its cap; and a `styles.xml` near the tree cap too. Run it under
/// `ulimit -v 3000000`.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_every_budget_at_once() {
    let polygon = r#"<draw:regular-polygon svg:width="1cm" svg:height="1cm" draw:corners="154999" draw:concave="true" draw:sharpness="50%"/>"#;
    every_budget("every budget", &format!("{polygon}{polygon}"));
}

/// Every budget at once, with the model's items taken by the costliest of the kinds adds (shapes of 10,000 equations) in place of the two
/// polygons.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_every_budget_at_once_with_equations() {
    let list: String = (0..10_000)
        .map(|i| {
            let f = if i < 2 { "1".to_owned() } else { format!("(?f{}+?f{})*0.5+$0", i - 1, i - 2) };
            format!(r#"<draw:equation draw:name="f{i}" draw:formula="{f}"/>"#)
        })
        .collect();
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="1" draw:enhanced-path="M 0 0 L ?f9999 0">{list}</draw:enhanced-geometry></draw:custom-shape>"#
    );
    every_budget("every budget, equations", &shape.repeat(14));
}

/// Every budget at once with long formulas (the review's worst mix, which the other cases did not hold): 116 shapes of one equation of
/// 131 KB take the items to 19.8 million of the 20 million, and the rest of the page, the text and the picture follow; at 118 the cap
/// refuses the file.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_every_budget_at_once_with_long_formulas() {
    // The number of shapes may be named (`DIACRAFT_LONG_SHAPES`) to move the mix between the long formulas and the rest of the page.
    let shapes = std::env::var("DIACRAFT_LONG_SHAPES").ok().and_then(|v| v.parse().ok()).unwrap_or(116);
    let (class, peak) = every_budget_with_long_formulas(shapes);
    assert!(peak < 2_800_000, "{peak} kB");
    eprintln!("result {class:?}");
}

/// Every budget at once, with the model's items taken by long formulas: custom shapes each with one equation of 131,069 bytes (a balanced
/// sum of 2^15 ones, 65,535 nodes, 3.2 MB of tree), the review's input B and the costliest cells the model holds for their items. `shapes`
/// of them go before the rest of the page's items. Run it under `ulimit -v 3000000`.
fn every_budget_with_long_formulas(shapes: usize) -> (Option<&'static str>, u64) {
    let sum = crate::read_formula_costs::balanced(15);
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?f0 0"><draw:equation draw:name="f0" draw:formula="{sum}"/></draw:enhanced-geometry></draw:custom-shape>"#
    );
    every_budget(&format!("every budget, {shapes} shapes of a 131 KB equation"), &shape.repeat(shapes))
}

/// The every-budget case with `heavy` as the page's costliest items.
fn every_budget(name: &str, heavy: &str) -> (Option<&'static str>, u64) {
    let frame = r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/big.png"/></draw:frame>"#;
    let spaces =
        format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"#, r#"<text:s text:c="65536"/>"#.repeat(1_023));
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:filler""#);
    // The re-review's font family too: a face of a 1,000,000-character family named by 4,000 common styles.
    d.fonts = format!(r#"<style:font-face style:name="F" svg:font-family="{}"/>"#, "f".repeat(1_000_000));
    let named: String = (0..4_000)
        .map(|k| format!(r#"<style:style style:name="P{k}" style:family="paragraph"><style:text-properties style:font-name="F"/></style:style>"#))
        .collect();
    d.common = format!("{}{named}", filler(diacraft_odg::limits::MAX_READ_XML_NODES / 2 - 20_000));
    // The big picture shown by four frames, and each of the three others by one.
    let frames = frame.repeat(4) + &["b", "c", "d"].iter().map(|n| frame.replace("big", n)).collect::<String>();
    d.pages = format!(
        r#"<draw:page draw:name="p" draw:master-page-name="Default">{frames}{heavy}{spaces}{}</draw:page>{}"#,
        unshared_rects(SHAPES_AT_ONCE),
        filler(FILLER_AT_ONCE)
    );
    // The package is at its 1 GiB, and **all of it is parts the read reads**: a read inflates only the parts the drawing names (plan
    //), so 720 MB of parts nothing names cost nothing (this case measured 1.98 GB with them, against 2.66 GB with pictures that frames show).
    let extra = ["Pictures/big.png", "Pictures/b.png", "Pictures/c.png", "Pictures/d.png"];
    let bytes = part_by_part(d, &extra, &|name| picture(if name.ends_with("big.png") { 250_000_000 } else { 240_000_000 }));
    measured(name, &bytes)
}

/// The media budget at once: three picture parts of 240 MB each shown, and two inline pictures of 100 MB of base64 in a `content.xml`
/// near its part cap, with both trees near their cap and the text at its cap: the package at its 1 GiB, and 870 MB of media.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_media_at_once() {
    let frame = |name: &str| format!(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/{name}"/></draw:frame>"#);
    // A PNG signature, then zeros: "iVBORw" and then "A"s, in base64.
    let inline = format!(
        r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image><office:binary-data>iVBORw{}</office:binary-data></draw:image></draw:frame>"#,
        "A".repeat(100_000_000 - 6)
    );
    let spaces =
        format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"#, r#"<text:s text:c="65536"/>"#.repeat(1_023));
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:filler""#);
    d.common = filler(diacraft_odg::limits::MAX_READ_XML_NODES / 2 - 10_000);
    d.pages = format!(
        r#"<draw:page draw:name="p" draw:master-page-name="Default">{}{}{}{inline}{inline}{spaces}</draw:page>{}"#,
        frame("a.png"),
        frame("b.png"),
        frame("c.png"),
        filler(FILLER_AT_ONCE)
    );
    drop(inline);
    let extra = ["Pictures/a.png", "Pictures/b.png", "Pictures/c.png"];
    let bytes = part_by_part(d, &extra, &|_| picture(240_000_000));
    measure("media at once", &bytes);
}

/// The re-review's font family: one `style:font-face` of a 1,000,000-character family, named by 1,000 or 4,000 common paragraph styles, by
/// 4,000 rectangles each on an automatic style of its own, and by 65,000 common styles (near the style cap). It aborted at 4,000.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_font_family() {
    let family = "f".repeat(1_000_000);
    for (name, styles, rects) in
        [("1,000 styles", 1_000, 0), ("4,000 styles", 4_000, 0), ("4,000 rectangles", 0, 4_000), ("65,000 styles", 65_000, 0)]
    {
        let mut d = Drawing::a4();
        d.fonts = format!(r#"<style:font-face style:name="F" svg:font-family="{family}"/>"#);
        d.common = (0..styles)
            .map(|k| format!(r#"<style:style style:name="P{k}" style:family="paragraph"><style:text-properties style:font-name="F"/></style:style>"#))
            .collect();
        let auto: String = (0..rects)
            .map(|k| format!(r#"<style:style style:name="gr{k}" style:family="graphic"><style:text-properties style:font-name="F" fo:font-size="{}pt"/></style:style>"#, 8 + k % 7))
            .collect();
        let shapes: String = (0..rects)
            .map(|k| format!(r#"<draw:rect draw:style-name="gr{k}" svg:width="1cm" svg:height="1cm"><text:p>x</text:p></draw:rect>"#))
            .collect();
        measure(&format!("font family, {name}"), &d.auto(&auto).page(&shapes).bytes());
    }
}

/// The stand-in for a path-heavy drawing: `draw:path` shapes of 46 commands each, as many as the census's largest page holds
/// shapes (35,191), and the most of them the item cap reads.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_path_heavy_drawing() {
    let path = |k: usize| {
        let d: String = (0..45).map(|j| format!(" L{} {}", (j * 7 + k) % 1000, (j * 13) % 1000)).collect();
        format!(r#"<draw:path svg:x="{}mm" svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 1000 1000" svg:d="M0 0{d}"/>"#, k % 200)
    };
    let drawing = |n: usize| -> Vec<u8> {
        let shapes: String = (0..n).map(path).collect();
        Drawing::a4().page(&shapes).bytes()
    };
    measure("35,191 paths of 46 commands", &drawing(35_191));
    let (mut lo, mut hi) = (1usize, 35_191usize);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if read(&drawing(mid), &Limits::default()).is_ok() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    eprintln!("paths of 46 commands: {lo} read, {hi} refused");
}

/// How many unshared rectangles and filler elements the every-budget case holds, chosen to bring the items and the XML near their caps.
pub(crate) const SHAPES_AT_ONCE: usize = 2_700;
pub(crate) const FILLER_AT_ONCE: usize = 1_750_000;

/// Every property the reader maps, with a value it parses: the three properties elements of a style that holds all of them (59 values).
pub fn every_property() -> String {
    let graphic = [
        ("draw:fill", "solid"),
        ("draw:fill-color", "#112233"),
        ("draw:opacity", "50%"),
        ("draw:fill-gradient-name", "g"),
        ("draw:fill-hatch-solid", "true"),
        ("draw:stroke", "dash"),
        ("draw:stroke-dash", "d"),
        ("svg:stroke-width", "1pt"),
        ("svg:stroke-color", "#445566"),
        ("svg:stroke-opacity", "50%"),
        ("svg:stroke-linecap", "round"),
        ("draw:marker-start", "m"),
        ("draw:marker-end", "m"),
        ("draw:marker-start-width", "1cm"),
        ("draw:marker-end-width", "1cm"),
        ("draw:shadow", "visible"),
        ("draw:shadow-color", "#000000"),
        ("draw:shadow-opacity", "30%"),
        ("draw:shadow-offset-x", "1mm"),
        ("draw:shadow-offset-y", "1mm"),
        ("draw:textarea-vertical-align", "middle"),
        ("fo:padding", "1mm"),
        ("fo:padding-top", "1mm"),
        ("fo:padding-bottom", "1mm"),
        ("fo:padding-left", "1mm"),
        ("fo:padding-right", "1mm"),
        ("style:writing-mode", "lr-tb"),
    ];
    let paragraph = [
        ("fo:text-align", "center"),
        ("fo:text-align-last", "end"),
        ("fo:margin-left", "1cm"),
        ("fo:margin-right", "1cm"),
        ("fo:text-indent", "1cm"),
        ("fo:margin-top", "1mm"),
        ("fo:margin-bottom", "1mm"),
        ("fo:line-height", "120%"),
        ("style:tab-stop-distance", "1cm"),
    ];
    let text = [
        ("fo:font-size", "12pt"),
        ("fo:font-weight", "bold"),
        ("style:font-weight-complex", "bold"),
        ("fo:font-style", "italic"),
        ("style:font-style-complex", "italic"),
        ("style:text-underline-style", "solid"),
        ("style:text-underline-type", "double"),
        ("style:text-line-through-style", "solid"),
        ("style:text-line-through-type", "single"),
        ("style:text-overline-style", "solid"),
        ("fo:font-variant", "normal"),
        ("fo:text-transform", "none"),
        ("style:text-position", "super 58%"),
        ("fo:color", "#778899"),
        ("fo:letter-spacing", "0.1cm"),
        ("style:text-scale", "90%"),
        ("style:font-name", "F"),
        ("fo:font-family", "'Free Serif'"),
        ("style:font-name-asian", "F"),
        ("style:font-family-asian", "Noto"),
        ("style:font-name-complex", "F"),
        ("style:font-family-complex", "Noto"),
    ];
    let element = |name: &str, props: &[(&str, &str)]| {
        let attrs: String = props.iter().map(|(n, v)| format!(r#" {n}="{v}""#)).collect();
        format!("<style:{name}{attrs}/>")
    };
    format!("{}{}{}", element("graphic-properties", &graphic), element("paragraph-properties", &paragraph), element("text-properties", &text))
}

/// Style properties at the reader's tree cap: 60,000 styles that each hold every property the reader maps (a typed value beside each text),
/// about 3.5 million of them, and a shape on one of them. Run it under `ulimit -v 3000000`.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_style_properties_at_the_tree_cap() {
    let props = every_property();
    let common: String = (0..60_000).map(|k| format!(r#"<style:style style:name="P{k}" style:family="paragraph">{props}</style:style>"#)).collect();
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="F" svg:font-family="Free Serif"/>"#.to_owned();
    d.common = common;
    let shapes = r#"<draw:rect draw:text-style-name="P7" svg:width="1cm" svg:height="1cm"><text:p>x</text:p></draw:rect>"#;
    measure("60,000 styles of 59 properties", &d.page(shapes).bytes());
}

/// Equations at the item cap: shapes each holding 10,000 equations of a formula that names the two before it, until the cap refuses
/// the file. An equation costs a `User` row, a formula cell and the text, besides the element kept in the shape's passthrough.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_equations_at_the_item_cap() {
    let list: String = (0..10_000)
        .map(|i| {
            let f = if i < 2 { "1".to_owned() } else { format!("(?f{}+?f{})*0.5+$0", i - 1, i - 2) };
            format!(r#"<draw:equation draw:name="f{i}" draw:formula="{f}"/>"#)
        })
        .collect();
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="1" draw:enhanced-path="M 0 0 L ?f9999 0">{list}</draw:enhanced-geometry></draw:custom-shape>"#
    );
    measure("70 shapes of 10,000 equations", &Drawing::a4().page(&shape.repeat(70)).bytes());
}

/// Arcs whose parameters are all formulas, the costliest cells a path makes, at the item cap.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_arcs_of_formulas_at_the_item_cap() {
    let arcs = "T ?f0 ?f1 ?f2 ?f3 0 90 ".repeat(40_000);
    let eq: String = (0..4).map(|i| format!(r#"<draw:equation draw:name="f{i}" draw:formula="{}"/>"#, 5_000 + i * 100)).collect();
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 {arcs}">{eq}</draw:enhanced-geometry></draw:custom-shape>"#
    );
    // Six of them are read (the table in `limits.rs` states their items and peak); the seventh takes the items past the cap.
    measure("6 shapes of 40,000 arcs of formulas", &Drawing::a4().page(&shape.repeat(6)).bytes());
    measure("7 shapes of 40,000 arcs of formulas", &Drawing::a4().page(&shape.repeat(7)).bytes());
}

/// The review's input A: the first point of a path is a sum of 131,069 bytes and each of k closes draws a line back to it.
/// The file is 3 KB; the closes once parsed the sum again each, and 1,000 of them aborted the process.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_a_long_formula_closed_a_thousand_times() {
    let sum = crate::read_formula_costs::balanced(15);
    for k in [100usize, 1_000, 4_000] {
        let path = format!("M {sum} 0 {}", "L 0 0 Z ".repeat(k));
        let shape = format!(
            r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="{path}"/></draw:custom-shape>"#
        );
        let (class, peak) = measured(&format!("{k} closes of a 131 KB point"), &Drawing::a4().page(&shape).bytes());
        assert_eq!(class, None, "{k}");
        assert!(peak < 1_000_000, "{k}: {peak} kB");
    }
}

/// The review's input B: shapes each with one equation of 131 KB, as many as the part cap allows. The trees are charged by their nodes, and
/// the items refuse the file long before the trees fill the memory.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_shapes_each_with_a_long_equation() {
    let sum = crate::read_formula_costs::balanced(15);
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?f0 0"><draw:equation draw:name="f0" draw:formula="{sum}"/></draw:enhanced-geometry></draw:custom-shape>"#
    );
    for n in [800usize, 1_000, 1_900] {
        let bytes = Drawing::a4().page(&shape.repeat(n)).bytes();
        let (class, peak) = measured(&format!("{n} shapes of a 131 KB equation"), &bytes);
        assert_eq!(class, Some("too_many_items"), "{n}");
        assert!(peak < 2_000_000, "{n}: {peak} kB");
    }
}

/// The narrow re-review's input for the compiled form of an equation: one custom shape with 1,900 equations, each a flat chain `1+1+...+1` of
/// 131,069 bytes (a 1.2 MB file, a 250 MB content part). The formula crate refuses such a chain as too deep, so no tree is built, and each
/// equation compiles to 131,069 operations of 16 bytes held with the others until the shape's equations are evaluated. Charged by the
/// bytes it holds the file is refused at the cap; charged by its text it aborted the process.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_one_shape_of_flat_chain_equations() {
    let chain = "1+".repeat(65_534) + "1";
    assert_eq!(chain.len(), 131_069);
    let equations: String = (0..1_900).map(|i| format!(r#"<draw:equation draw:name="f{i}" draw:formula="{chain}"/>"#)).collect();
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L 1 1">{equations}</draw:enhanced-geometry></draw:custom-shape>"#
    );
    let bytes = Drawing::a4().page(&shape).bytes();
    drop(shape);
    let (class, peak) = measured("one shape of 1,900 flat chains of 131 KB", &bytes);
    assert_eq!(class, Some("too_many_items"));
    assert!(peak < 2_000_000, "{peak} kB");
}

/// The narrow re-review's arcs of no sweep: 3,400 shapes of 100 `T` arcs each, six parameters of 119 characters that sum references to one
/// equation and the same expression for both angles (a 4.6 MB file, a 248 MB content part). No arc makes a row; each pays for its
/// parameters, 12 items, and the expressions are compiled once (the work was 7.5 s and 21% of the cap, before it was charged).
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_and_time_arcs_of_no_sweep() {
    let sum = "?g0+".repeat(29) + "?g0";
    let arc = format!("T {sum} {sum} {sum} {sum} {sum} {sum} ");
    let path = format!("M 0 0 {}", arc.repeat(100));
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="{path}"><draw:equation draw:name="g0" draw:formula="1"/></draw:enhanced-geometry></draw:custom-shape>"#
    );
    // The control: parameters that are plain numbers of the same length, which nothing compiles; what the read costs besides the parameters.
    let zeros = format!("0.{}", "0".repeat(117));
    let control = shape.replace(&sum, &zeros);
    for (name, text) in [("3,400 shapes of 100 arcs of no sweep", &shape), ("the same with plain numbers", &control)] {
        let bytes = Drawing::a4().page(&text.repeat(3_400)).bytes();
        let started = std::time::Instant::now();
        let (class, peak) = measured(name, &bytes);
        eprintln!("{name}: time {:?}", started.elapsed());
        assert_eq!(class, None);
        assert!(peak < 1_000_000, "{peak} kB");
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
    }
}

/// A group of unshared rectangles, and a page of connectors glued to a shape, at the item cap.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_a_group_and_connectors_at_the_item_cap() {
    measure("a group of 400,000 unshared rectangles", &Drawing::a4().page(&format!("<draw:g>{}</draw:g>", unshared_rects(400_000))).bytes());
    let connector = r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:start-shape="a" draw:start-glue-point="2" draw:end-shape="a" draw:end-glue-point="5"><text:p>t</text:p></draw:connector>"#;
    measure(
        "250,000 connectors",
        &Drawing::a4().page(&format!(r#"<draw:rect xml:id="a" svg:width="1cm" svg:height="1cm"/>{}"#, connector.repeat(250_000))).bytes(),
    );
}

/// Custom shapes with a glue list, a path of every command and text, at the item cap.
#[test]
#[ignore = "a memory measurement, run by hand one per process"]
fn memory_custom_shapes_of_every_row_kind_at_the_item_cap() {
    let points: String = (0..2_000).map(|i| format!("{} {}", i * 10, i * 5)).collect::<Vec<_>>().join(" ");
    let path = "M 0 0 L 100 100 C 1 2 3 4 5 6 Q 1 2 3 4 U 10800 10800 100 100 0 360 W 0 0 100 100 10 10 50 50 ".repeat(40);
    let shape = |k: usize| {
        format!(
            r#"<draw:custom-shape svg:x="{}mm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="1 2" draw:glue-points="{points}" draw:text-areas="0 0 21600 10800" draw:enhanced-path="{path}"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry><text:p>t</text:p></draw:custom-shape>"#,
            k % 200
        )
    };
    let shapes: String = (0..3_000).map(shape).collect();
    measure("3,000 custom shapes of every row kind", &Drawing::a4().page(&shapes).bytes());
}
