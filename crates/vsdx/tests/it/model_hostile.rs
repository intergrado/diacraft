//! The crafted inputs of (the plan's done criteria): what it costs the model writer to write the models that make it dear, **counted in the units its loops walk**
//! and, behind `#[ignore]`, timed beside a yardstick; and the memory a save of large parts takes, in a child process under `ulimit -v 3000000`.
//!
//! **What is counted** (`diacraft_vsdx::work`, which the crate's own tests turn on): the parts the writer looked at to plan and to write them, the relationship rows it made a row of or looked at, the
//! bytes of held parts it handed to the package (written from the model's own bytes), the rows of types it decided, the names it tried for a new part and the ids it tried for a new row. A count does not move
//! with the load on the machine; each bound below is the measured figure with a little room, and a loop that walked all the parts for each part, or all the rows for each row, is far past it.
//!
//! **The yardsticks.** For many parts, writing the same parts through the container's own `PackageWriter`; for a drawing of a thousand pages, the splice save of the same drawing (an edit to one page).

use std::io::Write as _;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{Document, Format, Part, PartRole, Rel, Shape, ShapeList, ShapeType, VsdxHeld};
use diacraft_opc::{Limits as OpcLimits, Package, PackageWriter};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteError, WriteOptions, work, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), content_unchanged: true, ..WriteOptions::default() }
}

fn try_write(doc: &Document) -> Result<diacraft_vsdx::Written, WriteError> {
    write_with(doc, None, &options())
}

/// A held part of these bytes, typed as a `.bin` part, with these relationships from it.
fn part(name: &str, bytes: &[u8], rels: Vec<Rel>) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from(name),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Other,
        manifest: diacraft_doc::RawAttrs::new(),
        rels: Arc::new(rels),
        bytes: Arc::from(bytes),
    })
}

fn rel(source: &str, id: &str, target: &str) -> Rel {
    Rel { source: Arc::from(source), id: Arc::from(id), kind: Arc::from("urn:t"), target: Arc::from(target), external: false }
}

/// A document that was read from a package (it holds `VsdxHeld`) with `n` held parts of ten bytes, `/p/n<i>.bin`.
fn with_parts(n: usize, chain: bool) -> Document {
    let mut doc = Document::new_blank();
    doc.vsdx = Some(Arc::new(VsdxHeld::default()));
    for i in 0..n {
        let name = format!("/p/n{i}.bin");
        let rels = if chain && i + 1 < n { vec![rel(&name, "rId1", &format!("n{}.bin", i + 1))] } else { Vec::new() };
        doc.parts.push(part(&name, &[7u8; 10], rels));
    }
    doc
}

// ------------------------------------------------------------------------------------------------------------------------- many parts

#[test]
fn fifty_thousand_held_parts_are_refused_in_words_before_one_is_looked_at() {
    let doc = with_parts(50_000, false);
    work::reset();
    let refused = try_write(&doc).expect_err("a package holds at most 50,000 parts, and the drawing's own are among them");
    assert!(matches!(refused, WriteError::Limit(what) if what.contains("parts")), "{refused}");
    assert_eq!(refused.class(), "write_limit");
    assert_eq!(work::model_parts_planned(), 0, "the count a model states is looked at before any of what it counts");
    // Under that count, the parts the writer makes beside them (and the relationships parts they have) are counted before any is written.
    let doc = with_parts(49_996, false);
    work::reset();
    let refused = try_write(&doc).expect_err("the drawing's own parts do not fit beside them");
    assert!(matches!(refused, WriteError::Limit(what) if what.contains("parts")), "{refused}");
    assert!(work::model_parts_planned() <= 49_996 + 8, "{} parts looked at", work::model_parts_planned());
    assert_eq!(work::model_bytes_written(), 0, "nothing is written");
}

#[test]
fn the_most_held_parts_that_fit_beside_the_drawings_own_are_written_and_read_back() {
    let doc = with_parts(49_990, false);
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(back.parts.len(), 49_990);
    assert!(report.stats.parts <= 50_000, "{} parts", report.stats.parts);
}

#[test]
fn forty_thousand_held_parts_are_looked_at_once_each_and_written_from_their_own_bytes() {
    let n = 40_000;
    let doc = with_parts(n, false);
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let planned = work::model_parts_planned();
    eprintln!(
        "[model] {n} held parts: {planned} parts planned, {} rows, {} bytes, {} types",
        work::model_rows_walked(),
        work::model_bytes_written(),
        work::model_types_decided()
    );
    // Each part is looked at to claim its name and once to be written, and the drawing's own parts beside them.
    assert!(planned >= 2 * n as u64 && planned <= 2 * n as u64 + 60, "{planned}");
    assert_eq!(work::model_bytes_written(), 10 * n as u64, "each part's bytes once, from the model's own");
    assert!(work::model_types_decided() <= n as u64 + 30, "{} types decided", work::model_types_decided());
    assert!(work::model_rows_walked() <= 40, "{} rows", work::model_rows_walked());
    assert!(work::name_probes() <= 40, "{} names tried", work::name_probes());
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(back.parts.len(), n);
    assert_eq!(report.held_bytes, 10 * n as u64);
    // The parts are typed by one `Default` row and not by a row each.
    let pkg = Package::read(&written.bytes, &OpcLimits::default()).unwrap();
    assert!(pkg.content_types().overrides.len() < 20, "{} Override rows", pkg.content_types().overrides.len());
}

#[test]
fn twenty_thousand_held_parts_that_each_name_the_next_are_written_one_row_each() {
    let n = 20_000;
    let doc = with_parts(n, true);
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let rows = work::model_rows_walked();
    eprintln!("[model] {n} chained parts: {rows} rows, {} parts", work::model_parts_planned());
    // A row is made once and written once: the chain's 19,999 rows, and the drawing's own.
    assert!(rows >= n as u64 - 1 && rows <= n as u64 + 40, "{rows}");
    assert!(work::model_parts_planned() <= 2 * n as u64 + 60);
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.parts.iter().filter(|p| p.rels.len() == 1).count(), n - 1);
}

// ----------------------------------------------------------------------------------------------------------------- many relationships

#[test]
fn a_million_held_relationship_rows_are_refused_in_words_before_one_is_made() {
    let mut doc = with_parts(0, false);
    let name = "/p/hub.bin";
    let rows: Vec<Rel> = (0..1_000_000).map(|i| rel(name, &format!("r{i}"), "t")).collect();
    doc.parts.push(part(name, &[], rows));
    work::reset();
    let refused = try_write(&doc).expect_err("a million rows from one part");
    assert!(matches!(refused, WriteError::Limit(what) if what.contains("relationships")), "{refused}");
    assert_eq!(work::model_rows_walked(), 0, "refused by the count, before any row is looked at");
    // Just under that count, the rows are too many bytes for a relationships part (4 MiB) and are refused by the bytes the model states, in one pass.
    let mut doc = with_parts(0, false);
    let rows: Vec<Rel> = (0..99_000).map(|i| rel(name, &format!("r{i}"), "a-longer-target-than-t")).collect();
    doc.parts.push(part(name, &[], rows));
    work::reset();
    let refused = try_write(&doc).expect_err("the rows are more bytes than a relationships part may be");
    assert!(matches!(refused, WriteError::Limit(what) if what.contains("larger")), "{refused}");
    assert_eq!(work::model_rows_walked(), 0);
}

#[test]
fn the_most_rows_a_relationships_part_may_have_are_written_and_read_back() {
    // 40,000 rows of the size a row has in a file are 2 MB of XML: under the 4 MiB a relationships part may be.
    let mut doc = with_parts(0, false);
    let name = "/p/hub.bin";
    let n = 40_000;
    let rows: Vec<Rel> = (0..n).map(|i| rel(name, &format!("r{i}"), "t")).collect();
    doc.parts.push(part(name, &[], rows));
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    assert!(work::model_rows_walked() >= n as u64 && work::model_rows_walked() <= n as u64 + 40, "{}", work::model_rows_walked());
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.parts.iter().find(|p| &*p.name == name).map(|p| p.rels.len()), Some(n));
}

// ------------------------------------------------------------------------------------------------------------------------ many pages

/// A drawing of `pages` pages of three shapes each, as the package of a file.
fn pages_package(pages: usize) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    for p in 1..=pages {
        let shapes: String = (1..=3).map(|s| shape(s, &cell("PinX", &format!("{p}.{s}")))).collect();
        builder = builder.page(PageSpec::new(u32::try_from(p).unwrap(), &format!("Page {p}")).shapes(&shapes));
    }
    builder.build()
}

#[test]
fn a_thousand_pages_are_planned_and_written_in_one_pass_each() {
    let bytes = pages_package(1_000);
    let (doc, _) = read_ok(&bytes);
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let (parts, rows, probes, ids) = (work::model_parts_planned(), work::model_rows_walked(), work::name_probes(), work::id_scans());
    eprintln!("[model] 1,000 pages read: {parts} parts, {rows} rows, {probes} names tried, {ids} ids tried");
    // Pages keep their names and ids: a name or an id is not looked for.
    assert!(parts <= 2 * 1_000 + 20 && rows <= 1_000 + 40, "{parts} parts, {rows} rows");
    assert!(probes <= 10 && ids <= 10, "{probes} names, {ids} ids");
    assert_eq!(read_ok(&written.bytes).0.pages.len(), 1_000);
}

#[test]
fn a_thousand_pages_a_model_made_are_given_names_and_ids_by_one_walk_each() {
    // Pages with no part and no row: every name and every id is new, and found by one walk up the numbers, not by one for each page.
    let mut doc = Document::new_blank();
    doc.vsdx = Some(Arc::new(VsdxHeld::default()));
    for _ in 0..999 {
        doc.add_page(None).unwrap();
    }
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let (probes, ids) = (work::name_probes(), work::id_scans());
    eprintln!("[model] 1,000 new pages: {probes} names tried, {ids} ids tried, {} rows", work::model_rows_walked());
    assert!(probes <= 1_000 + 20, "{probes} names tried");
    assert!(ids <= 1_000 + 20, "{ids} ids tried");
    assert_eq!(read_ok(&written.bytes).0.pages.len(), 1_000);
}

#[test]
fn a_thousand_pages_that_name_one_part_write_one_part() {
    let bytes = pages_package(2);
    let (mut doc, _) = read_ok(&bytes);
    // Every page names the part of the first and holds its shapes: one list, which a thousand pages share.
    let first = Arc::clone(doc.pages.get(0).unwrap());
    for id in 3..=1_000u32 {
        let mut page = (*first).clone();
        page.id = id;
        page.name = Arc::from(format!("Page {id}").as_str());
        page.rel_id = Some(Arc::from(format!("rId{id}").as_str()));
        doc.pages.push(Arc::new(page));
    }
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let page_parts = Package::read(&written.bytes, &OpcLimits::default()).unwrap().parts().filter(|n| n.starts_with("/visio/pages/page")).count();
    eprintln!("[model] 1,000 pages naming one part: {page_parts} page parts, {} parts planned", work::model_parts_planned());
    assert!(page_parts <= 3, "{page_parts} page parts (the first page's, the second's, and no more)");
    assert!(work::model_parts_planned() <= 1_100, "{}", work::model_parts_planned());
    assert_eq!(read_ok(&written.bytes).0.pages.len(), 1_000);
}

// ------------------------------------------------------------------------------------------------------------------------ chained costs

/// One picture shown by every one of `n` shapes of a page: the shape's `Foreign` data names its row, and holds the one picture.
fn picture_page(n: usize) -> Document {
    let media = Arc::new(diacraft_doc::Media {
        part: Arc::from("/visio/media/one.png"),
        content_type: Arc::from("image/png"),
        bytes: Arc::from(&[0x89u8, b'P', b'N', b'G', 1, 2][..]),
    });
    let foreign = Arc::new(diacraft_doc::ForeignData {
        attrs: diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("CompressionType", "PNG")]),
        rel_id: Some(Arc::from("rId1")),
        picture: Some(media),
        ..diacraft_doc::ForeignData::default()
    });
    let mut doc = Document::new_blank();
    doc.vsdx = Some(Arc::new(VsdxHeld::default()));
    let shapes: Vec<Arc<Shape>> =
        (1..=n as u32).map(|id| Arc::new(Shape { id, kind: ShapeType::Foreign, foreign: Some(Arc::clone(&foreign)), ..Shape::default() })).collect();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.shapes = ShapeList::from_vec(shapes);
    doc
}

#[test]
fn one_picture_that_every_shape_names_is_one_part_and_one_row() {
    let doc = picture_page(5_000);
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    let rows = work::model_rows_walked();
    eprintln!("[model] one picture in 5,000 shapes: {rows} rows, {} parts", work::model_parts_planned());
    assert!(rows <= 40, "{rows} rows");
    let pkg = Package::read(&written.bytes, &OpcLimits::default()).unwrap();
    assert_eq!(pkg.parts().filter(|n| n.starts_with("/visio/media/")).count(), 1);
    assert_eq!(pkg.rels("/visio/pages/page1.xml").unwrap().len(), 1);
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.pages.get(0).unwrap().shapes.len(), 5_000);
}

/// A page whose shapes are `depth` groups in a chain, each group's `Shapes` element holding an attribute and a comment of its own.
fn chain_package(depth: usize) -> Vec<u8> {
    let mut xml = String::new();
    for i in 1..=depth {
        xml.push_str(&format!("<Shape ID=\"{i}\" Type=\"Group\"><Shapes x:level=\"{i}\" xmlns:x=\"urn:x\"><!--c{i}-->"));
    }
    for _ in 1..=depth {
        xml.push_str("</Shapes></Shape>");
    }
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&xml)).build()
}

#[test]
fn nested_groups_that_each_hold_a_slot_are_written_at_the_depth_the_reader_reads_and_refused_past_it() {
    let (doc, _) = read_ok(&chain_package(90));
    work::reset();
    let written = try_write(&doc).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("[model] 90 nested groups with a slot each: {} parts, {} rows", work::model_parts_planned(), work::model_rows_walked());
    let d = diacraft_vsdx::compare_content(&chain_package(90), &written.bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(d.same(), "{:?}", d.shown);
    // A model nested past the depth the reader reads is refused in words, and not written.
    let mut shape = Shape::new(1);
    for i in 2..=250u32 {
        let mut outer = Shape::new(i);
        outer.kind = ShapeType::Group;
        outer.children = ShapeList::from_vec(vec![Arc::new(shape)]);
        shape = outer;
    }
    let mut deep = Document::new_blank();
    deep.vsdx = Some(Arc::new(VsdxHeld::default()));
    Arc::make_mut(deep.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let refused = try_write(&deep).expect_err("250 levels of groups");
    assert!(refused.to_string().contains("nested"), "{refused}");
    // 150 levels are inside the model's cap and past what a part holds: the file made does not read back, and the save says so.
    let (mut shape, mut level) = (Shape::new(1), 2u32);
    while level <= 150 {
        let mut outer = Shape::new(level);
        outer.kind = ShapeType::Group;
        outer.children = ShapeList::from_vec(vec![Arc::new(shape)]);
        shape = outer;
        level += 1;
    }
    Arc::make_mut(deep.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let refused = try_write(&deep).expect_err("150 levels of groups are 300 levels of XML");
    assert!(matches!(refused.class(), "write_limit" | "write_self_check"), "{refused}");
}

// ------------------------------------------------------------------------------------------------- timed beside the yardstick (ignored)

/// The time of the best of `runs` of `f`.
fn best_of(runs: usize, mut f: impl FnMut()) -> std::time::Duration {
    (0..runs)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .min()
        .unwrap_or_default()
}

#[test]
#[ignore = "timed: run beside the yardstick by hand"]
fn timed_forty_thousand_parts_beside_the_package_writer() {
    let n = 40_000;
    let doc = with_parts(n, false);
    let ours = best_of(3, || {
        try_write(&doc).unwrap();
    });
    let yard = best_of(3, || {
        let types = b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"bin\" ContentType=\"application/octet-stream\"/></Types>";
        let mut w = PackageWriter::new(types).unwrap();
        for i in 0..n {
            w.add(&format!("/p/n{i}.bin"), &[7u8; 10]).unwrap();
        }
        w.finish().unwrap();
    });
    eprintln!("[timed] {n} held parts: {ours:?} against {yard:?} for the container's writer alone");
    assert!(ours < yard * 4, "{ours:?} against {yard:?}");
}

#[test]
#[ignore = "timed: run beside the yardstick by hand"]
fn timed_a_thousand_pages_beside_the_splice_save() {
    let bytes = pages_package(1_000);
    let (doc, _) = read_ok(&bytes);
    let ours = best_of(3, || {
        try_write(&doc).unwrap();
    });
    // The splice save of the same drawing, with one page edited.
    let mut edited = doc.clone();
    let slot = edited.pages.get_mut(0).unwrap();
    let page = Arc::make_mut(slot);
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet)
        .cells
        .insert(diacraft_doc::CellName::new("PinX"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("9")));
    let yard = best_of(3, || {
        write_with(&edited, Some(&OriginalBytes { bytes: &bytes, baseline: Some(&doc) }), &options()).unwrap();
    });
    eprintln!("[timed] 1,000 pages: {ours:?} for the model save against {yard:?} for the splice save");
    assert!(ours < yard * 4, "{ours:?} against {yard:?}");
}

// ------------------------------------------------------------------------------------------------------ memory: a child under the limit

const SCENARIO: &str = "DIACRAFT_VSDXOUT_SCENARIO";
#[cfg(target_os = "linux")]
const LIMIT_KIB: u64 = 3_000_000;

fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// `n` bytes no compression makes smaller (a generator of ours: the bytes of a held part that is already compressed).
fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(n);
    out
}

fn big_parts(count: usize, each: usize) -> Document {
    let mut doc = Document::new_blank();
    doc.vsdx = Some(Arc::new(VsdxHeld::default()));
    for i in 0..count {
        doc.parts.push(part(&format!("/big/{i}.bin"), &noise(each, i as u64 + 1), Vec::new()));
    }
    doc
}

/// The shapes of a diagram over three pages, as a package.
fn diagram(n: usize) -> Vec<u8> {
    let pages = 3usize;
    let per = n.div_ceil(pages);
    let mut builder = VsdxBuilder::new();
    for p in 0..pages {
        let body: String = (p * per..((p + 1) * per).min(n)).map(diagram_shape).collect();
        builder = builder.page(PageSpec::new(u32::try_from(p + 1).unwrap(), &format!("Page {}", p + 1)).shapes(&body));
    }
    builder.build()
}

/// A shape of the kind a diagram has: fourteen cells, two formulas, a path of five rows and a text.
fn diagram_shape(i: usize) -> String {
    let (x, y) = (format!("{}.{}", i % 997, i % 89), format!("{}.{}", i / 997, i % 83));
    let cells = format!(
        "<Cell N=\"PinX\" V=\"{x}\"/><Cell N=\"PinY\" V=\"{y}\"/><Cell N=\"Width\" V=\"1\"/><Cell N=\"Height\" V=\"0.75\"/><Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><Cell N=\"Angle\" V=\"0\"/><Cell N=\"FlipX\" V=\"0\"/><Cell N=\"FlipY\" V=\"0\"/><Cell N=\"ResizeMode\" V=\"0\"/><Cell N=\"LineWeight\" V=\"0.01\"/><Cell N=\"LineColor\" V=\"0\"/><Cell N=\"FillForegnd\" V=\"1\"/><Cell N=\"FillPattern\" V=\"1\"/>"
    );
    let points = [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")];
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(ix, (t, px, py))| {
            format!("<Row T=\"{t}\" IX=\"{}\"><Cell N=\"X\" V=\"1\" F=\"{px}\"/><Cell N=\"Y\" V=\"1\" F=\"{py}\"/></Row>", ix + 1)
        })
        .collect();
    format!(
        "<Shape ID=\"{}\" Type=\"Shape\" Name=\"Process\" NameU=\"Process\">{cells}<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{rows}</Section><Text><cp IX=\"0\"/>Label</Text></Shape>",
        i + 1
    )
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs a scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("the scenario");
    let mut parts = scenario.split(':');
    let kind = parts.next().unwrap_or("");
    let a: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let b: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let doc = match kind {
        "parts" => big_parts(a, b),
        _ => {
            // A drawing of `a` shapes over three pages with three held parts of a mebibyte: the model of a file that came through `.diacraft`.
            let bytes = diagram(a);
            let (mut doc, _) = read_ok(&bytes);
            drop(bytes);
            for i in 0..3 {
                doc.parts.push(part(&format!("/held/{i}.bin"), &noise(1 << 20, i as u64 + 9), Vec::new()));
            }
            doc
        }
    };
    let started = Instant::now();
    let outcome = write_with(&doc, None, &options());
    let words = match &outcome {
        Ok(w) => format!("written {} bytes", w.bytes.len()),
        Err(e) => format!("refused:{} {e}", e.class()),
    };
    println!("OUTCOME {words}");
    println!("TIME {:?}", started.elapsed());
    println!("VMPEAK {}", status_kib("VmPeak"));
    println!("model: no abort");
    let _ = std::io::stdout().flush();
}

/// Runs a scenario in a child under `ulimit -v 3000000`: it ends well (no abort), and says how.
#[cfg(target_os = "linux")]
fn under_the_limit(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = format!("ulimit -v {LIMIT_KIB} && exec \"$0\" --exact model_hostile::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("model: no abort"), "{scenario}: the child did not end well: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("OUTCOME") || l.contains("VMPEAK") || l.starts_with("TIME")).collect::<Vec<_>>().join(" | "));
    text
}

#[cfg(target_os = "linux")]
fn vmpeak(text: &str) -> u64 {
    text.lines().find_map(|l| l.strip_prefix("VMPEAK ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

#[test]
#[cfg(target_os = "linux")]
fn held_parts_too_large_to_write_and_read_back_are_refused_in_words_and_not_an_abort() {
    // Three parts of 256 MiB that no compression makes smaller: the file and the package the read-back inflates are each that large, beside the model.
    let text = under_the_limit("parts:3:268435456");
    assert!(text.contains("OUTCOME refused:write_limit"), "{text}");
    assert!(text.contains("too large to be written and read back"), "{text}");
}

#[test]
#[cfg(target_os = "linux")]
fn held_parts_that_fit_are_written_and_read_back_under_the_address_limit() {
    let text = under_the_limit("parts:6:41943040");
    assert!(text.contains("OUTCOME written"), "{text}");
    assert!(vmpeak(&text) < LIMIT_KIB / 100 * 90, "the address space reached {} KiB of the {LIMIT_KIB}", vmpeak(&text));
}

#[test]
#[cfg(target_os = "linux")]
fn parts_that_fill_what_the_check_admits_are_written_and_read_back_and_a_little_more_is_refused_before_a_byte_is_written() {
    // 671 MB of parts in a few: admitted, and the address space it takes (measured: 2.52 GB of the 3.07) is under the limit. The same 838,860,800 bytes in twenty parts of 40 MiB and in ten
    // of 80 MiB were admitted by the check of a save that read the parts into a document, and the read-back refused them after the whole file was made, at the limit of the address space: now the
    // save says so in words, and makes nothing.
    let fits = under_the_limit("parts:5:134217728");
    assert!(fits.contains("OUTCOME written"), "{fits}");
    assert!(vmpeak(&fits) < LIMIT_KIB / 100 * 96, "the address space reached {} KiB of the {LIMIT_KIB}", vmpeak(&fits));
    for scenario in ["parts:20:41943040", "parts:10:83886080"] {
        let text = under_the_limit(scenario);
        assert!(text.contains("OUTCOME refused:write_limit"), "{scenario}: {text}");
        assert!(text.contains("too large to be written and read back"), "{scenario}: {text}");
    }
}

#[test]
#[cfg(target_os = "linux")]
fn a_drawing_of_seventy_six_thousand_shapes_with_held_parts_is_written_under_the_address_limit() {
    // The save-peak check of the engine admits the splice save of 76,000 shapes over three pages (it refuses from 78,000): the model save of the same drawing, with parts held, must fit where
    // that does, and peak no higher than the splice save did (2,894 MiB).
    let text = under_the_limit("diagram:76000");
    assert!(text.contains("OUTCOME written"), "{text}");
    assert!(vmpeak(&text) <= 2_894 * 1024, "the address space reached {} KiB, and the splice save of the drawing reached 2,894 MiB", vmpeak(&text));
}
