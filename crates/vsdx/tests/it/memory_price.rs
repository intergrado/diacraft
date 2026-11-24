//! The read's memory, held to its price and to the 3 GB of an address-space limit.
//!
//! **A scenario is a file made in code, and it is read in a process of its own.** The parent makes the bytes (no limit on it) and starts the test binary again as
//! a child (`child`), writing the length of the file and then the file to its input, under `ulimit -v 3000000` where the case says so; the child reads it and
//! prints what the read cost. The child's figures are the peak the read added to its process (`VmHWM` after a reset, less what was resident before), the address
//! space the process reached (`VmPeak`), and the bytes the read charged to its pool at the most (`Stats::memory_peak`). **A read is charged for at least what it
//! adds** (`the_price_*` below: the bytes charged and the package are never under the growth of the process, for each kind of file that was measured), and **a
//! read that would pass the pool is refused in words** (`under_the_limit_*`: every hostile case ends as a read or a refusal with its class and never as an
//! abort, which is a child that did not end well). The cost of each charge, counted, is in `memory_charges.rs`. **What the model keeps is held to its charge in
//! `memory_counted.rs`, counted by the allocator**: the growth of the process misses a vector's room that nothing has touched and moves with the platform's
//! allocator, so these hold the peak of a whole read, which no count of what is kept sees, and the counted tests hold each kind.
//!
//! The figures printed are the ones the table of `limits.rs` is made from: `probe` runs scenarios by hand, free and under the limit, and prints a line each.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use diacraft_testkit::package::{
    CT_MASTER, CT_MASTERS, MasterSpec, NS_2012, NS_R, PageSpec, Pkg, VIS, VsdxBuilder, cell, cell_f, cell_u, nested_shapes, shape, shape_with,
};
use diacraft_vsdx::{Limits, read_partial};

/// The environment variables of a child run by hand: a file to read in place of the input, and the limits to apply (`field=value,...`).
const INPUT: &str = "DIACRAFT_VSDX_MEMORY_INPUT";
const LIMITS: &str = "DIACRAFT_VSDX_MEMORY_LIMITS";

/// A field of `/proc/self/status` in KiB.
fn status_kib(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status.lines().find_map(|l| l.strip_prefix(field)).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()).unwrap_or(0)
}

/// What the child prints, parsed.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub ended_well: bool,
    pub stdout: String,
}

impl Outcome {
    /// The number printed after `key` (`ADDED 123`).
    pub fn number(&self, key: &str) -> u64 {
        self.stdout
            .lines()
            .find_map(|l| l.find(key).map(|at| &l[at + key.len()..]))
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or_else(|| panic!("no {key} in {}", self.stdout))
    }

    /// The word printed after `key`.
    pub fn word(&self, key: &str) -> String {
        self.stdout
            .lines()
            .find_map(|l| l.find(key).map(|at| &l[at + key.len()..]))
            .and_then(|v| v.split_whitespace().next().map(str::to_owned))
            .unwrap_or_else(|| panic!("no {key} in {}", self.stdout))
    }
}

/// Runs the child on `bytes`, under `ulimit -v limit_kib` (`None`: no limit), with `limits` (`default`, or a comma-separated `field=value` list the child
/// applies to the default limits). The file goes in by the child's input, its length first, so that the child holds exactly the file and nothing is written to disk.
pub(crate) fn run_child(name: &str, bytes: &[u8], limit_kib: Option<u64>, limits: &str) -> Outcome {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let ulimit = limit_kib.map_or(String::new(), |kib| format!("ulimit -v {kib} && "));
    let script = format!("{ulimit}exec \"$0\" --exact memory_price::child --ignored --nocapture --test-threads=1");
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(script)
        .arg(exe)
        .env(LIMITS, limits)
        .env_remove(INPUT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the test binary");
    let mut input = child.stdin.take().expect("the child's input");
    // A child that aborts while it reads closes its input: the write fails, and the child's own end is what is reported.
    let _ = input.write_all(&(bytes.len() as u64).to_le_bytes());
    let _ = input.write_all(bytes);
    drop(input);
    let out = child.wait_with_output().expect("the child's end");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        eprintln!("[child {name}] did not end well: {:?}\n{stdout}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    }
    Outcome { ended_well: out.status.success(), stdout }
}

/// The file a child reads: the one named in the environment (a run by hand), else what the parent writes to its input.
fn child_input() -> Vec<u8> {
    if let Ok(path) = std::env::var(INPUT) {
        return std::fs::read(path).expect("the input read");
    }
    let mut stdin = std::io::stdin().lock();
    let mut len = [0u8; 8];
    stdin.read_exact(&mut len).expect("the length of the file");
    let len = usize::try_from(u64::from_le_bytes(len)).expect("a length that fits");
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).expect("room for the file");
    bytes.resize(len, 0);
    stdin.read_exact(&mut bytes).expect("the file");
    bytes
}

/// The child: reads its input and prints what the read cost. `ignored`, so that a plain run of the suite does not run it.
#[test]
#[ignore = "run as a child of the tests that measure a read"]
fn child() {
    if std::env::var(LIMITS).is_err() {
        return;
    }
    let bytes = child_input();
    let mut limits = Limits::default();
    for pair in std::env::var(LIMITS).unwrap_or_default().split(',').filter(|p| !p.is_empty() && *p != "default") {
        let (field, value) = pair.split_once('=').expect("a field and a value");
        let value: u64 = value.parse().expect("a number");
        match field {
            "max_memory" => limits.max_memory = value,
            "max_items" => limits.max_items = value as usize,
            "max_cells" => limits.max_cells = value as usize,
            "max_shapes" => limits.max_shapes = value as usize,
            "max_media_bytes" => limits.max_media_bytes = value,
            other => panic!("no limit {other}"),
        }
    }
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    let before = status_kib("VmRSS:");
    let started = std::time::Instant::now();
    let (result, report) = read_partial(&bytes, &limits);
    let took = started.elapsed();
    let after_hwm = status_kib("VmHWM:");
    let peak = status_kib("VmPeak:");
    let outcome = match &result {
        Ok(_) => "read".to_owned(),
        Err(e) => format!("refused:{}", e.class()),
    };
    let s = &report.stats;
    println!();
    println!("OUTCOME {outcome}");
    println!("ADDED {}", after_hwm.saturating_sub(before));
    println!("VMPEAK {peak}");
    println!("FILE {}", bytes.len());
    println!("PACKAGE {}", s.uncompressed_bytes);
    println!("CHARGED_PEAK {}", s.memory_peak);
    println!("CHARGED_END {}", s.memory_bytes);
    println!("ITEMS {}", s.items);
    println!("CELLS {}", s.cells);
    println!("SHAPES {}", s.shapes);
    println!("MAX_XML_ITEMS {}", s.max_xml_items);
    println!("MAX_TREE_BYTES {}", s.max_tree_bytes);
    println!("MEDIA_LEFT_OUT {}", report.warning_count("media_over_budget"));
    println!("HELD_PARTS {}", report.held_parts);
    println!("HELD_BYTES {}", report.held_bytes);
    println!("MILLIS {}", took.as_millis());
    println!("FORMULA_NODES {}", s.formula_nodes);
    if let Err(e) = &result {
        println!("MESSAGE {e}");
    }
    drop(result);
}

// ----------------------------------------------------------------------------------------------------------------------------- the files

fn pages_of(shapes: &str, pages: u32) -> Vec<u8> {
    let mut b = VsdxBuilder::new();
    for k in 1..=pages {
        b = b.page(PageSpec::new(k, "P").shapes(shapes));
    }
    b.build()
}

/// The shape a diagram is made of, as much as a drawing of boxes holds: fourteen cells (six of them formulas), a path of five rows of two cells, a text. **Its sheet and its path are
/// its own** (a place and a value in the path that depend on `id`): a reader that shares what is equal shares nothing of these, which is the worst a file can make.
pub(crate) fn typical_shape(id: u32) -> String {
    let direct = [
        cell_u("PinX", &format!("{id}.5"), "IN"),
        cell_u("PinY", "2.25", "IN"),
        cell_u("Width", "1", "IN"),
        cell_u("Height", "0.75", "IN"),
        cell_f("LocPinX", "0.5", "Width*0.5"),
        cell_f("LocPinY", "0.375", "Height*0.5"),
        cell("Angle", "0"),
        cell("FlipX", "0"),
        cell("FlipY", "0"),
        cell_u("LineWeight", "0.01", "IN"),
        cell("LineColor", "0"),
        cell("FillForegnd", "#ffffff"),
        cell_f("TxtPinX", "0.5", "Width*0.5"),
        cell_f("TxtPinY", "0.375", "Height*0.5"),
    ]
    .concat();
    let rows: String = (1..=5)
        .map(|r| {
            format!(
                "<Row T=\"LineTo\" IX=\"{r}\"><Cell N=\"X\" V=\"{r}\" U=\"IN\" F=\"Width*{r}\"/><Cell N=\"Y\" V=\"{id}\" U=\"IN\" F=\"Height*1\"/></Row>"
            )
        })
        .collect();
    let geometry = format!("<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{rows}</Section>");
    shape_with(
        id,
        " Type=\"Shape\" Name=\"Box\" NameU=\"Box\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"",
        &format!("{direct}{geometry}<Text><cp IX=\"0\"/>Hello world</Text>"),
    )
}

/// `pages` pages that hold `per_page` typical shapes each.
pub(crate) fn typical_pages(per_page: usize, pages: u32) -> Vec<u8> {
    let mut b = VsdxBuilder::new();
    for k in 1..=pages {
        // An `ID` of its own for each shape of each page, so that no page is another's.
        let shapes: String = (0..per_page as u32).map(|i| typical_shape((k - 1) * per_page as u32 + i)).collect();
        b = b.page(PageSpec::new(k, "P").shapes(&shapes));
    }
    b.build()
}

/// A masters index of `n` rows that all name one master part of one shape, with the relationships part that says so.
fn masters_pkg(n: u32) -> Pkg {
    let mut rows = String::new();
    let mut rels = String::new();
    for i in 0..n {
        rows.push_str(&format!("<Master ID=\"{i}\" NameU=\"M{i}\" UniqueID=\"{{GUID-{i}}}\"><Rel r:id=\"rId{}\"/></Master>", 100_000 - i));
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{VIS}master\" Target=\"master1.xml\"/>", 100_000 - i));
    }
    let masters = format!("<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Masters>");
    let rels_xml = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>"
    );
    let master = format!("<MasterContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></MasterContents>", shape(4, ""));
    Pkg::new()
        .part("/visio/masters/masters.xml", CT_MASTERS, masters)
        .rel("/visio/document.xml", "rId60", &format!("{VIS}masters"), "masters/masters.xml")
        .part("/visio/masters/master1.xml", CT_MASTER, master)
        .part("/visio/masters/_rels/masters.xml.rels", "application/vnd.openxmlformats-package.relationships+xml", rels_xml)
}

/// A page part whose one shape holds `n` chains of 190 nested elements, one child each: the tree that costs the most for its items, because a list of children has
/// room for four and an element that has one child holds the room of four.
fn chains_part(n: usize) -> String {
    let chain = format!("{}{}", "<a>".repeat(190), "</a>".repeat(190));
    format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"1\">{}</Shape></Shapes></PageContents>", chain.repeat(n))
}

/// `count` characters of the CJK block, three bytes each in UTF-8 and two in UTF-16, from a set of sixteen at random so that a deflater cannot take the file under
/// the ratio the container allows.
fn cjk_noise(count: usize) -> String {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut text = String::with_capacity(count.saturating_mul(3));
    for _ in 0..count {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        text.extend(char::from_u32(0x4E00 + ((seed >> 40) as u32 & 0xF)));
    }
    text
}

/// `text` as UTF-16 with a byte order mark, little endian.
fn utf16_le(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// A page part that is one text of `count` CJK characters and a line break written as CR LF, as a UTF-16 part of the package: the part that is `2 * count` bytes
/// in the package and 3 * `count` bytes of text for the parse.
fn utf16_text_page(pkg: Pkg, page: u32, count: usize) -> Pkg {
    let wanted = format!("pages/page{page}.xml");
    pkg.map_parts(|name, bytes| {
        if name.ends_with(&wanted) {
            let text = format!("<PageContents xmlns=\"{NS_2012}\">{}\r\nend</PageContents>", cjk_noise(count));
            *bytes = utf16_le(&text);
        }
    })
}

/// Two pages of a thousand shapes with `cells` empty cells each, then a page of 250 MB of UTF-16 text.
fn utf16_text_after_cells(cells: usize) -> Vec<u8> {
    let cells: String = (0..cells).map(|i| format!("<Cell N=\"C{i}\"/>")).collect();
    let shapes: String = (0..1_000).map(|i| shape(i, &cells)).collect();
    let mut b = VsdxBuilder::new();
    for k in 1..=2u32 {
        b = b.page(PageSpec::new(k, "P").shapes(&shapes));
    }
    utf16_text_page(b.page(PageSpec::new(3, "L").part("<PageContents/>")).pkg(), 3, 125_000_000).build()
}

/// The files the cases read, by name. `typical_p<shapes>_<pages>` is that many pages of that many shapes of a diagram each.
pub(crate) fn scenario(name: &str) -> Vec<u8> {
    if let Some(rest) = name.strip_prefix("typical_p") {
        let (per_page, pages) = rest.split_once('_').expect("shapes and pages");
        return typical_pages(per_page.parse().expect("a number of shapes"), pages.parse().expect("a number of pages"));
    }
    match name {
        "unknown_million" => VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &"<z/>".repeat(1_000_000)))).build(),
        "unknown_one_part_of_six_million" => VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &"<z/>".repeat(5_990_000)))).build(),
        "unknown_four_parts" => {
            let part = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></PageContents>", shape(1, &"<z/>".repeat(4_990_000)));
            let mut b = VsdxBuilder::new();
            for k in 1..=4u32 {
                b = b.page(PageSpec::new(k, "P").part(&part));
            }
            b.build()
        }
        "empty_cells_million" => {
            let cells: String = (0..1_000).map(|i| format!("<Cell N=\"C{i}\"/>")).collect();
            let shapes: String = (0..1_000).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "empty_cells_four_parts" => {
            let cells: String = (0..2_490).map(|i| format!("<Cell N=\"C{i}\"/>")).collect();
            let shapes: String = (0..1_000).map(|i| shape(i, &cells)).collect();
            pages_of(&shapes, 4)
        }
        "empty_rows_million" => {
            let rows: String = (0..1_000).map(|i| format!("<Row N=\"r{i}\"/>")).collect();
            let section = format!("<Section N=\"User\">{rows}</Section>");
            let shapes: String = (0..1_000).map(|i| shape(i, &section)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "valued_cells" => {
            let cells: String = (0..1_000).map(|i| format!("<Cell N=\"C{i}\" V=\"{i}\" U=\"IN\" F=\"Width*{i}\"/>")).collect();
            let shapes: String = (0..700).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "kept_attributes" => {
            let cells: String = (0..1_000)
                .map(|i| format!("<Cell N=\"C{i}\" V=\"{i}\" a=\"aaaa\" b=\"bbbbbbbb\" c=\"cccccccccccc\" d=\"d\" e=\"eeeeeeeeeeeeeeeeeeee\"/>"))
                .collect();
            let shapes: String = (0..400).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "empty_shapes_million" => {
            let shapes: String = (0..1_000_000u32).map(|i| format!("<Shape ID=\"{i}\"/>")).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "sections" => {
            let inner: String = (0..8_192).map(|i| format!("<Section N=\"S{i}\"/>")).collect();
            let shapes: String = (0..100).map(|i| format!("<Shape ID=\"{i}\">{inner}</Shape>")).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "text" => {
            let text: String = (0..2_000).map(|i| format!("<cp IX=\"{i}\"/>some words here <fld IX=\"{i}\">F{i}</fld><pp IX=\"{i}\"/>")).collect();
            let shapes: String = (0..200).map(|i| shape(i, &format!("<Text>{text}</Text>"))).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "connects" => {
            let connects: String = (0..500_000)
                .map(|i| format!("<Connect FromSheet=\"{i}\" FromCell=\"BeginX\" FromPart=\"9\" ToSheet=\"1\" ToCell=\"PinX\" ToPart=\"3\"/>"))
                .collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, "")).connects(&connects)).build()
        }
        "pages_sharing_one_part" => super::timing::pages_sharing_one_part(1_000, 1_500),
        "masters" => VsdxBuilder::new().without_masters_part().with(masters_pkg(30_000)).build(),
        "master_instances" => {
            let master = MasterSpec::new(1, "M").shapes(&typical_shape(1));
            let members: String = (0..20_000).map(|i| shape_with(i + 100, " Master=\"1\"", "")).collect();
            VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&members)).build()
        }
        "long_formulas" => {
            let cells: String =
                (0..1_000).map(|i| format!("<Cell N=\"C{i}\" V=\"1\" F=\"(Width+Height)*(PinX-PinY)+(Width+Height)*(PinX-PinY)+{i}\"/>")).collect();
            let shapes: String = (0..800).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "pictures" => {
            // Three pictures of 90 MiB each, shown by a shape each: the bytes are what a photograph is, noise.
            let mut seed = 0x9E37_79B9_7F4A_7C15u64;
            let mut noise = |n: usize| -> Vec<u8> {
                (0..n)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        (seed >> 24) as u8
                    })
                    .collect()
            };
            let one = |k: u32| {
                format!("<Shape ID=\"{k}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId{k}\"/></ForeignData></Shape>")
            };
            let shapes: String = (1..=3u32).map(one).collect();
            let mut page = PageSpec::new(1, "P").shapes(&shapes);
            let mut pkg = Pkg::new();
            for k in 1..=3u32 {
                page = page.rel(
                    &format!("rId{k}"),
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                    &format!("../media/image{k}.png"),
                );
                pkg = pkg.part(&format!("/visio/media/image{k}.png"), "image/png", noise(90 * 1024 * 1024));
            }
            VsdxBuilder::new().page(page).with(pkg).build()
        }
        "long_values_600" | "long_values_900" => {
            // Cells of nearly 1 MiB each (an attribute may be a MiB), 150 to a page of 150 MiB: the pages are the package, and each value is held by the tree
            // and by the model while its part is read.
            let pages: u32 = if name == "long_values_600" { 4 } else { 6 };
            let value = "9".repeat(1024 * 1024 - 64);
            let cells: String = (0..150).map(|i| format!("<Cell N=\"C{i}\" V=\"{value}\"/>")).collect();
            let mut b = VsdxBuilder::new();
            for k in 1..=pages {
                b = b.page(PageSpec::new(k, "P").shapes(&shape(1, &cells)));
            }
            b.pkg().stored().build()
        }
        "formula_strings_3" | "formula_strings_8" => {
            // Cells whose formula is one string literal of a million characters: the text is in the part, in the tree, in the formula, and in the one node of its tree
            // (the strings of a tree are copies of the text), 120 to a page of 120 MB.
            let pages: u32 = if name == "formula_strings_3" { 3 } else { 8 };
            let literal = "x".repeat(1_000_000);
            let cells: String = (0..120).map(|i| format!("<Cell N=\"C{i}\" V=\"1\" F=\"&quot;{literal}&quot;\"/>")).collect();
            let mut b = VsdxBuilder::new();
            for k in 1..=pages {
                b = b.page(PageSpec::new(k, "P").shapes(&shape(1, &cells)));
            }
            b.build()
        }
        "document_passthrough" => {
            VsdxBuilder::new().document_extra(&"<z/>".repeat(3_000_000)).page(PageSpec::new(1, "P").shapes(&"<Shape ID=\"1\"/>".repeat(10))).build()
        }
        "cell_extra" => {
            let cells: String = (0..1_000)
                .map(|i| {
                    if i % 2 == 0 {
                        format!("<Cell N=\"C{i}\" V=\"1\">some text of the cell {i}</Cell>")
                    } else {
                        format!("<Cell N=\"C{i}\" V=\"1\"><RefBy ID=\"{i}\" Del=\"1\"/><?NewValue V?></Cell>")
                    }
                })
                .collect();
            let shapes: String = (0..400).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "deep_groups" => {
            let inner = shape(9999, &"<Cell N=\"X\" V=\"1\"/>".repeat(8));
            let members: String = (0..2_000).map(|i| nested_shapes(60, &shape(i, "")) + &inner).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&members)).build()
        }
        "styles_65k" => {
            let styles: String = (1..=65_535u32).map(|i| format!("<StyleSheet ID=\"{i}\" Name=\"S{i}\" NameU=\"S{i}\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Cell N=\"LineWeight\" V=\"0.01\"/></StyleSheet>")).collect();
            VsdxBuilder::new().more_styles(&styles).build()
        }
        "foreign_many" => {
            let one = |k: u32| {
                format!(
                    "<Shape ID=\"{k}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\" CompressionType=\"PNG\"><Rel r:id=\"rId{k}\"/></ForeignData></Shape>"
                )
            };
            let shapes: String = (1..=200_000u32).map(one).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        // The parts of a diagram's shape, each alone in 80,000 shapes (four pages of 20,000), to see which price is high.
        "typ_bare" | "typ_direct" | "typ_direct_f" | "typ_rows" | "typ_rows_f" | "typ_text" => {
            let direct = |formulas: bool| {
                [
                    cell_u("PinX", "1.5", "IN"),
                    cell_u("PinY", "2.25", "IN"),
                    cell_u("Width", "1", "IN"),
                    cell_u("Height", "0.75", "IN"),
                    if formulas { cell_f("LocPinX", "0.5", "Width*0.5") } else { cell("LocPinX", "0.5") },
                    if formulas { cell_f("LocPinY", "0.375", "Height*0.5") } else { cell("LocPinY", "0.375") },
                    cell("Angle", "0"),
                    cell("FlipX", "0"),
                    cell("FlipY", "0"),
                    cell_u("LineWeight", "0.01", "IN"),
                    cell("LineColor", "0"),
                    cell("FillForegnd", "#ffffff"),
                    if formulas { cell_f("TxtPinX", "0.5", "Width*0.5") } else { cell("TxtPinX", "0.5") },
                    if formulas { cell_f("TxtPinY", "0.375", "Height*0.5") } else { cell("TxtPinY", "0.375") },
                ]
                .concat()
            };
            let rows = |formulas: bool| -> String {
                let body: String = (1..=5)
                    .map(|r| {
                        if formulas {
                            format!("<Row T=\"LineTo\" IX=\"{r}\"><Cell N=\"X\" V=\"{r}\" U=\"IN\" F=\"Width*{r}\"/><Cell N=\"Y\" V=\"1\" U=\"IN\" F=\"Height*1\"/></Row>")
                        } else {
                            format!("<Row T=\"LineTo\" IX=\"{r}\"><Cell N=\"X\" V=\"{r}\" U=\"IN\"/><Cell N=\"Y\" V=\"1\" U=\"IN\"/></Row>")
                        }
                    })
                    .collect();
                format!("<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{body}</Section>")
            };
            let inner = |id: u32| match name {
                "typ_bare" => shape_with(id, " Type=\"Shape\" Name=\"Box\" NameU=\"Box\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"", ""),
                "typ_direct" => shape(id, &direct(false)),
                "typ_direct_f" => shape(id, &direct(true)),
                "typ_rows" => shape(id, &rows(false)),
                "typ_rows_f" => shape(id, &rows(true)),
                _ => shape(id, "<Text><cp IX=\"0\"/>Hello world</Text>"),
            };
            let shapes: String = (0..20_000u32).map(inner).collect();
            pages_of(&shapes, 4)
        }
        // 200,000 cells in 200 shapes, with the formula `f` (or none): what a node of a tree costs is the difference between a file with and a file without.
        "fn_none" | "fn_refs" | "fn_calls" | "fn_nums" | "fn_names" => {
            let f = match name {
                "fn_none" => String::new(),
                "fn_refs" => " F=\"Sheet.1!Width+Sheet.2!Height+Sheet.3!PinX+Prop.A.B.Value\"".to_owned(),
                "fn_calls" => " F=\"IF(Width>1,MAX(Height,2),MIN(PinX,PinY))\"".to_owned(),
                "fn_nums" => " F=\"1+2+3+4+5+6+7+8+9+10+11+12\"".to_owned(),
                _ => " F=\"LongerNameOfACellThatIsHere+AnotherLongerNameOfACell+ThirdLongerNameOfACell\"".to_owned(),
            };
            let cells: String = (0..1_000).map(|i| format!("<Cell N=\"C{i}\" V=\"1\"{f}/>")).collect();
            let shapes: String = (0..200).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "typical_20k" => typical_pages(5_000, 4),
        "typical_60k" => typical_pages(20_000, 3),
        "typical_80k" => typical_pages(20_000, 4),
        "typical_100k" => typical_pages(20_000, 5),
        "typical_120k" => typical_pages(20_000, 6),
        "typical_140k" => typical_pages(20_000, 7),
        "gigabyte" | "gigabyte_dear_parts" => {
            // 16 page parts of some 63 MiB: a gigabyte of XML in all, a little under the package cap of 1 GiB. Each part is `shapes` shapes with two cells: 400,000 are 3.6
            // million items, which the pool left by a gigabyte of package (1.45 GB) cannot hold at the dearest a tree can be (1.3 GB and the room for the text): the first part is
            // refused before it is parsed (`gigabyte_dear_parts`). 200,000 are 1.8 million items, 0.7 GB at the bound: each is read until the pool is spent (`gigabyte`). The
            // shapes are padded by a name until the sixteen parts are 1,060,000,000 bytes (the numbers of the shapes are of different lengths, so the padding is found by making
            // one part and measuring it).
            let shapes: u32 = if name == "gigabyte" { 200_000 } else { 400_000 };
            let make = |pad: usize| -> String {
                let name = "n".repeat(pad);
                (0..shapes).map(|i| shape_with(i, &format!(" Name=\"{name}\""), &(cell("Width", "1") + &cell("Height", "2")))).collect()
            };
            let mut pad = 100;
            let mut one = make(pad);
            pad = (pad + 1_060_000_000 / 16 / shapes as usize).saturating_sub(one.len() / shapes as usize);
            one = make(pad);
            let mut b = VsdxBuilder::new();
            for i in 1..=16 {
                b = b.page(PageSpec::new(i, "P").shapes(&one));
            }
            b.build()
        }
        // Three parts of 256 MiB the drawing does not read, stored: a file of 805 MB held by the caller, inflated into a package of 805 MB, and each part copied into the
        // model as a part the document holds, which is 2.4 GB beside the process's own.
        "embeddings_three_stored" | "embeddings_five_of_200_stored" => {
            let (n, size) = if name == "embeddings_three_stored" { (3, 256 * 1024 * 1024) } else { (5, 200 * 1024 * 1024) };
            let zeros = vec![0u8; size];
            let mut pkg = Pkg::new();
            for k in 1..=n {
                pkg = pkg.part(&format!("/visio/embeddings/oleObject{k}.bin"), "application/vnd.openxmlformats-officedocument.oleObject", &zeros);
            }
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).with(pkg).pkg().stored().build()
        }
        "pictures_gigabyte" => {
            // Four pictures of 250 MiB that no compression takes anything from, stored: a file of a gigabyte, held by the caller, inflated into a package of a
            // gigabyte, and each picture copied into the model when a shape shows it.
            let mut seed = 0x2545_F491_4F6C_DD1Du64;
            let mut noise = |n: usize| -> Vec<u8> {
                (0..n)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        (seed >> 24) as u8
                    })
                    .collect()
            };
            let one = |k: u32| {
                format!("<Shape ID=\"{k}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId{k}\"/></ForeignData></Shape>")
            };
            let shapes: String = (1..=4u32).map(one).collect();
            let mut page = PageSpec::new(1, "P").shapes(&shapes);
            let mut pkg = Pkg::new();
            for k in 1..=4u32 {
                page = page.rel(
                    &format!("rId{k}"),
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                    &format!("../media/image{k}.png"),
                );
                pkg = pkg.part(&format!("/visio/media/image{k}.png"), "image/png", noise(250 * 1024 * 1024));
            }
            VsdxBuilder::new().page(page).with(pkg).pkg().stored().build()
        }
        "every_budget" => {
            // Every kind of thing a file can ask for, each in a part of its own and at a size that is a share of the pool: unknown elements kept in a part, empty
            // cells, cells with values, units and formulas, rows, the text of 200 shapes, the shapes of a diagram, connects, thirty thousand masters, twenty
            // thousand styles, two pictures of 100 MB, and a theme. The pool is spent in the last page, so every kind is in the model when it is refused.
            let cells: String = (0..300).map(|i| format!("<Cell N=\"C{i}\"/>")).collect();
            let empty_cells: String = (0..1_000).map(|i| shape(i, &cells)).collect();
            let valued: String = (0..300).map(|i| format!("<Cell N=\"C{i}\" V=\"{i}\" U=\"IN\" F=\"Width*{i}\"/>")).collect();
            let valued: String = (0..300).map(|i| shape(i, &valued)).collect();
            let rows: String = (0..300).map(|i| format!("<Row N=\"r{i}\"/>")).collect();
            let rows: String = (0..1_000).map(|i| shape(i, &format!("<Section N=\"User\">{rows}</Section>"))).collect();
            let text: String = (0..1_000).map(|i| format!("<cp IX=\"{i}\"/>some words <fld IX=\"{i}\">F{i}</fld>")).collect();
            let text: String = (0..200).map(|i| shape(i, &format!("<Text>{text}</Text>"))).collect();
            let diagram: String = (0..8_000u32).map(typical_shape).collect();
            let connects: String = (0..100_000)
                .map(|i| format!("<Connect FromSheet=\"{i}\" FromCell=\"BeginX\" FromPart=\"9\" ToSheet=\"1\" ToCell=\"PinX\" ToPart=\"3\"/>"))
                .collect();
            let mut seed = 0x3C6E_F372_FE94_F82Bu64;
            let mut noise = |n: usize| -> Vec<u8> {
                (0..n)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        (seed >> 24) as u8
                    })
                    .collect()
            };
            let picture = |k: u32| {
                format!("<Shape ID=\"{k}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId{k}\"/></ForeignData></Shape>")
            };
            let image = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
            let styles: String = (1..=20_000u32).map(|i| format!("<StyleSheet ID=\"{i}\" Name=\"S{i}\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Cell N=\"LineWeight\" V=\"0.01\"/></StyleSheet>")).collect();
            let theme = format!("<a:theme xmlns:a=\"urn:x\" name=\"{}\"><a:themeElements/></a:theme>", "t".repeat(1_000_000));
            let last = shape(1, &"<z/>".repeat(4_500_000));
            VsdxBuilder::new()
                .without_masters_part()
                .more_styles(&styles)
                .theme(theme.as_bytes())
                .with(masters_pkg(30_000))
                .page(PageSpec::new(1, "A").shapes(&shape(1, &"<z/>".repeat(1_500_000))))
                .page(PageSpec::new(2, "B").shapes(&empty_cells))
                .page(PageSpec::new(3, "C").shapes(&valued))
                .page(PageSpec::new(4, "D").shapes(&rows))
                .page(PageSpec::new(5, "E").shapes(&text))
                .page(PageSpec::new(6, "F").shapes(&diagram).connects(&connects))
                .page(PageSpec::new(7, "G").shapes(&(picture(1) + &picture(2))).rel("rId1", image, "../media/image1.png").rel(
                    "rId2",
                    image,
                    "../media/image2.png",
                ))
                .page(PageSpec::new(8, "H").shapes(&last))
                .with(Pkg::new().part("/visio/media/image1.png", "image/png", noise(100 * 1024 * 1024)).part(
                    "/visio/media/image2.png",
                    "image/png",
                    noise(100 * 1024 * 1024),
                ))
                .pkg()
                .stored()
                .build()
        }
        "chains_after_unknown_elements" => {
            // Seven pages of unknown elements (11 million of them: the pool is nearly spent on the model), then a page of 31,500 chains of 190 nested elements, one child
            // each: 5,985,000 items that cost 335 bytes each in the tree, where a bound of 224 an item let it be parsed (it aborted under the limit).
            let mut b = VsdxBuilder::new();
            for (k, count) in [2_000_000usize, 2_000_000, 2_000_000, 2_000_000, 1_000_000, 1_000_000, 1_000_000].into_iter().enumerate() {
                b = b.page(PageSpec::new(k as u32 + 1, "F").shapes(&shape(1, &"<z/>".repeat(count))));
            }
            b.page(PageSpec::new(8, "L").part(&chains_part(31_500))).build()
        }
        // Empty cells in two pages (1,250 or 1,750 in each of a thousand shapes: 2.5 million or 3.5 million), then a page of 125 million characters in UTF-16: 250 MB in the
        // package and 375 MB of text, which the parse decoded into a string of its own and copied again for the one line break written as CR LF, and nothing counted
        // (with 2.5 million cells it aborted under the limit).
        "utf16_text_after_cells_2500k" => utf16_text_after_cells(1_250),
        "utf16_text_after_cells_3500k" => utf16_text_after_cells(1_750),
        "utf16_text_alone" => utf16_text_page(VsdxBuilder::new().page(PageSpec::new(1, "L").part("<PageContents/>")).pkg(), 1, 125_000_000).build(),
        other => panic!("no scenario {other}"),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the probe

/// Makes the scenarios named in `DIACRAFT_VSDX_MEMORY_PROBE` and runs a child on each, free and under the limit, and prints one line each: the probe the table of
/// `limits.rs` is made from. Run by hand, in release.
#[test]
#[ignore = "a probe, run by hand in release"]
fn probe() {
    let Ok(names) = std::env::var("DIACRAFT_VSDX_MEMORY_PROBE") else { return };
    for name in names.split(',') {
        let bytes = scenario(name);
        for limit in [None, Some(3_000_000u64)] {
            let out = run_child(name, &bytes, limit, "default");
            let get = |key: &str| {
                if out.ended_well {
                    out.stdout.lines().find_map(|l| l.strip_prefix(key)).unwrap_or("?").trim().to_owned()
                } else {
                    "ABORT".to_owned()
                }
            };
            println!(
                "\nPROBE {name} limit={limit:?} outcome={} added={} vmpeak={} file={} package={} charged_peak={} charged_end={} items={} cells={} shapes={} xml_items={} tree={} millis={} nodes={}",
                get("OUTCOME "),
                get("ADDED "),
                get("VMPEAK "),
                get("FILE "),
                get("PACKAGE "),
                get("CHARGED_PEAK "),
                get("CHARGED_END "),
                get("ITEMS "),
                get("CELLS "),
                get("SHAPES "),
                get("MAX_XML_ITEMS "),
                get("MAX_TREE_BYTES "),
                get("MILLIS "),
                get("FORMULA_NODES "),
            );
        }
    }
}

/// Writes the files of the scenarios named in `DIACRAFT_VSDX_MEMORY_SAVE` to the folder `DIACRAFT_VSDX_MEMORY_DIR`, so that a child can be run on each by hand at
/// any limit (`DIACRAFT_VSDX_MEMORY_INPUT=file DIACRAFT_VSDX_MEMORY_LIMITS=default ... --exact memory_price::child`).
#[test]
#[ignore = "a probe, run by hand in release"]
fn save_inputs() {
    let (Ok(names), Ok(dir)) = (std::env::var("DIACRAFT_VSDX_MEMORY_SAVE"), std::env::var("DIACRAFT_VSDX_MEMORY_DIR")) else { return };
    for name in names.split(',') {
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.vsdx")), scenario(name)).expect("the input written");
    }
}

// ---------------------------------------------------------------------------------------------------------- the price is not under the growth

/// A kind of file read in a process of its own with no limit: **the bytes charged to the pool at the most, with the package they were read from, are not under
/// the growth of the process at its peak** (the package is inflated during the read, so it is in the growth), and not more than twice it either, so that a price
/// that has run away is found too. The figures of the report are these.
fn the_price_holds(name: &str, least_growth: u64) {
    let out = run_child(name, &scenario(name), None, "default");
    assert!(out.ended_well, "{name}: the child did not end well");
    assert_eq!(out.word("OUTCOME "), "read", "{name}: {}", out.stdout);
    let growth = out.number("ADDED ") * 1024;
    let charged = out.number("CHARGED_PEAK ") + out.number("PACKAGE ");
    // A list that grows holds the old beside the new for a moment, which the pool keeps back (`GROWTH_BYTES`) and the price does not hold.
    let growth_room = diacraft_vsdx::cost::GROWTH_BYTES;
    eprintln!(
        "[memory] {name}: the process grew {growth}, {charged} charged and held in the package ({:.2} times), {} items, {} cells, {} shapes, the largest tree {}",
        charged as f64 / growth as f64,
        out.number("ITEMS "),
        out.number("CELLS "),
        out.number("SHAPES "),
        out.number("MAX_TREE_BYTES ")
    );
    assert!(growth >= least_growth, "{name}: the file is too small to measure ({growth} bytes)");
    assert!(charged + growth_room >= growth, "{name}: {charged} bytes charged and held, and the process grew {growth}");
    assert!(charged <= growth.saturating_mul(2), "{name}: {charged} bytes charged and held, and the process grew only {growth}");
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_a_million_unknown_elements_is_not_under_what_they_add() {
    the_price_holds("unknown_million", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_a_million_empty_cells_is_not_under_what_they_add() {
    the_price_holds("empty_cells_million", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_a_million_empty_rows_is_not_under_what_they_add() {
    the_price_holds("empty_rows_million", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_a_million_empty_shapes_is_not_under_what_they_add() {
    the_price_holds("empty_shapes_million", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_cells_with_a_value_a_unit_and_a_formula_is_not_under_what_they_add() {
    the_price_holds("valued_cells", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_cells_that_keep_attributes_is_not_under_what_they_add() {
    the_price_holds("kept_attributes", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_sections_of_distinct_names_is_not_under_what_they_add() {
    the_price_holds("sections", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_text_in_many_parts_is_not_under_what_it_adds() {
    the_price_holds("text", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_cells_that_hold_text_and_markup_is_not_under_what_they_add() {
    the_price_holds("cell_extra", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_formulas_that_are_one_long_string_each_is_not_under_what_they_add() {
    the_price_holds("formula_strings_3", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_connects_is_not_under_what_they_add() {
    the_price_holds("connects", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_twenty_thousand_shapes_of_a_diagram_is_not_under_what_they_add() {
    the_price_holds("typical_20k", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_a_document_part_that_keeps_three_million_unknown_elements_is_not_under_what_it_adds() {
    the_price_holds("document_passthrough", 100_000_000);
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "the growth of the process is read from `/proc/self/status`, which macOS does not have")]
fn the_price_of_two_hundred_thousand_pictures_shown_and_not_there_is_not_under_what_they_add() {
    the_price_holds("foreign_many", 100_000_000);
}

// --------------------------------------------------------------------------------------------------------------- every case under the limit

/// The address space a read may reach, in KiB, and the limit a child is run under.
const LIMIT_KIB: u64 = 3_000_000;

/// A kind of file read in a child under `ulimit -v 3000000`: **it ends**, as a read or as a refusal in words with one of `classes`, and never as an abort, and
/// the address space it reached is under the limit. Whatever the child printed is the report's figures.
fn under_the_limit(name: &str, classes: &[&str]) -> Outcome {
    let out = run_child(name, &scenario(name), Some(LIMIT_KIB), "default");
    assert!(out.ended_well, "{name}: the child ended in an abort or a crash under `ulimit -v {LIMIT_KIB}`");
    let outcome = out.word("OUTCOME ");
    eprintln!(
        "[memory] {name}: {outcome}, address space {} KiB, {} items, {} cells, {} shapes, {} bytes charged at the most beside a file of {} and a package of {}",
        out.number("VMPEAK "),
        out.number("ITEMS "),
        out.number("CELLS "),
        out.number("SHAPES "),
        out.number("CHARGED_PEAK "),
        out.number("FILE "),
        out.number("PACKAGE ")
    );
    assert!(classes.contains(&outcome.as_str()), "{name}: ended as {outcome}, not one of {classes:?}: {}", out.stdout);
    // The limit is what the process cannot pass, so a peak under it says nothing; the budget leaves a share of it for what no price counts (the allocator's slack, the process's
    // own 143 MB, a list that grows beside its old self), and **a read that has spent more than 90 percent of the limit has spent that share and more**: the budget is
    // too high, or a charge is too low. The widest of these cases is 79 percent.
    assert!(
        out.number("VMPEAK ") < LIMIT_KIB / 100 * 90,
        "{name}: the address space reached {} KiB of the {LIMIT_KIB} the limit allows",
        out.number("VMPEAK ")
    );
    out
}

/// The refusal a read that would pass the pool is.
const REFUSED_FOR_MEMORY: &str = "refused:too_much_memory";

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn the_most_items_a_document_may_keep_in_four_parts_of_five_million_are_refused_in_words_under_the_address_limit() {
    let out = under_the_limit("unknown_four_parts", &[REFUSED_FOR_MEMORY]);
    assert!(out.stdout.contains("bytes of memory"), "{}", out.stdout);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn the_most_empty_cells_a_document_may_keep_in_four_parts_are_refused_in_words_under_the_address_limit() {
    // The reader shares what is equal: the empty sheets of these shapes are one, so the memory these cells take is less, and what refuses the read is the cap of cells.
    under_the_limit("empty_cells_four_parts", &[REFUSED_FOR_MEMORY, "refused:too_many_cells"]);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn the_worst_a_package_of_a_gigabyte_of_xml_costs_is_refused_in_words_under_the_address_limit() {
    let out = under_the_limit("gigabyte", &[REFUSED_FOR_MEMORY]);
    assert!(out.number("PACKAGE ") > 1_000_000_000, "the package is a gigabyte of XML: {}", out.number("PACKAGE "));
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_gigabyte_of_xml_in_parts_the_pool_cannot_hold_the_tree_of_is_refused_before_the_first_is_parsed_under_the_address_limit() {
    // 400,000 shapes in a part: the bound of the parse is more than the pool a gigabyte of package leaves, and nothing is made of the part.
    let out = under_the_limit("gigabyte_dear_parts", &[REFUSED_FOR_MEMORY]);
    assert!(out.number("PACKAGE ") > 1_000_000_000, "the package is a gigabyte of XML: {}", out.number("PACKAGE "));
    assert!(out.number("SHAPES ") == 0, "no part was read: {}", out.stdout);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_file_of_a_gigabyte_of_pictures_is_read_with_the_pictures_the_pool_can_hold_and_the_rest_left_out_under_the_address_limit() {
    let out = under_the_limit("pictures_gigabyte", &["read"]);
    // The pool, less the file and the package (1,048,583,071 and 1,048,580,054 bytes) and the 67.1 MB kept back for a list that grows, is 435.7 MB: one picture of
    // 262.1 MB is copied and the other three are not.
    assert_eq!(out.number("MEDIA_LEFT_OUT "), 3, "{}", out.stdout);
    // The three pictures the model did not copy are parts the document holds, and their bytes are the package's: held, and not counted a second time (F1 of
    // Before it they were copies, and the file was refused).
    assert_eq!(out.number("HELD_PARTS "), 3, "{}", out.stdout);
    assert!(out.number("HELD_BYTES ") >= 3 * 250 * 1024 * 1024, "{}", out.stdout);
}

/// Review focus 4 of the native format's plan: after a change the document holds every kept part inflated, and a file near the package cap could have aborted at open.
/// **The bytes of a held part are the package's, moved into the document and not copied beside it** (F1), so a file main reads is read: three parts of 256 MiB and five of
/// 200 MiB, stored, end as a read under `ulimit -v 3000000`, with their bytes held once.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn large_embeddings_open_or_refuse_in_a_child_under_ulimit() {
    let big: u64 = 256 * 1024 * 1024;
    let out = under_the_limit("embeddings_three_stored", &["read"]);
    assert_eq!((out.number("HELD_PARTS "), out.number("HELD_BYTES ")), (3, 3 * big), "{}", out.stdout);
    // And a package of five parts of 200 MiB, a gigabyte with its file: the file, the package and the copy of a part, one at a time, are 2.3 GB.
    let out = under_the_limit("embeddings_five_of_200_stored", &["read"]);
    assert_eq!((out.number("HELD_PARTS "), out.number("HELD_BYTES ")), (5, 5 * 200 * 1024 * 1024), "{}", out.stdout);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_hundred_thousand_shapes_of_a_diagram_are_refused_in_words_under_the_address_limit() {
    under_the_limit("typical_100k", &[REFUSED_FOR_MEMORY]);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn the_most_items_one_part_may_hold_all_kept_as_unknown_elements_are_read_under_the_address_limit() {
    under_the_limit("unknown_one_part_of_six_million", &["read"]);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn nine_hundred_values_of_a_mebibyte_in_six_pages_are_refused_as_xml_or_memory_under_the_address_limit() {
    under_the_limit("long_values_900", &["refused:too_much_xml", REFUSED_FOR_MEMORY]);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn nine_hundred_and_sixty_formulas_of_a_string_of_a_million_characters_are_read_or_refused_in_words_under_the_address_limit() {
    under_the_limit("formula_strings_8", &["read", REFUSED_FOR_MEMORY, "refused:too_much_xml"]);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn every_budget_at_once_ends_as_a_read_or_a_refusal_in_words_under_the_address_limit() {
    let out = under_the_limit("every_budget", &["read", REFUSED_FOR_MEMORY, "refused:too_many_items"]);
    // The file is made so that every kind of thing is in it: the budgets it passes are said by the figures.
    assert!(out.number("CELLS ") > 600_000 && out.number("SHAPES ") > 20_000, "{}", out.stdout);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn seven_pages_of_unknown_elements_and_a_page_of_chains_of_nested_elements_are_refused_in_words_under_the_address_limit() {
    let out = under_the_limit("chains_after_unknown_elements", &[REFUSED_FOR_MEMORY]);
    assert!(out.stdout.contains("bytes of memory"), "{}", out.stdout);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn two_and_a_half_million_empty_cells_and_a_page_of_utf16_text_of_250_megabytes_are_read_or_refused_in_words_under_the_address_limit() {
    // The pool after the cells holds the page's text and its copy, and the file is read: the parse held 375 MB of text twice, uncounted, and the process was over the limit.
    under_the_limit("utf16_text_after_cells_2500k", &["read", REFUSED_FOR_MEMORY]);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn three_and_a_half_million_empty_cells_and_a_page_of_utf16_text_of_250_megabytes_are_refused_in_words_under_the_address_limit() {
    // The pool after the cells does not hold the copy of the page's text and the room its tree can take for it (three times the 375 MB), and the part is refused before it
    // is decoded.
    // The reader shares what is equal: the empty sheets of the cells' shapes are one, the model is smaller, and the pool after them holds the copy of the page's
    // text: the file is read, within the address space, or refused in words.
    let out = under_the_limit("utf16_text_after_cells_3500k", &[REFUSED_FOR_MEMORY, "read"]);
    if out.word("OUTCOME ") != "read" {
        assert!(out.stdout.contains("bytes of memory"), "{}", out.stdout);
    }
    assert!(out.number("CELLS ") > 3_000_000, "the cells are all in the model before the page is refused: {}", out.stdout);
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_page_of_utf16_text_of_250_megabytes_is_read_under_the_address_limit() {
    under_the_limit("utf16_text_alone", &["read"]);
}
