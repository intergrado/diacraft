//! What a read is charged for the model it builds is at least what the model keeps, **counted**, for each kind of thing a file can repeat.
//!
//! `memory_price.rs` holds the charge of whole reads to the growth of the process's peak resident size, which is no measure of the model: it misses
//! a vector's room that nothing has touched, and moves with the platform's allocator (a page's connects kept 118 bytes each and the process grew by 52 to 98 of them,
//! by the machine). Here **the binary's allocator counts** (`GLOBAL` in `main.rs`): a child reads a file of `n` units of a kind and a file of none, keeps each
//! document until it is counted (the report is let go first: it is not the model), and prints the bytes and the allocations the document holds as the allocator was
//! asked for them, and what the read charged (`Stats::memory_bytes`, the pool's charge when the read is done, with the held parts' bytes). **The law is that the
//! charge of the units covers the bytes they keep, an eighth of them for the size classes of the app's allocator ([`CLASS_SHARE`], mimalloc)
//! and, for each allocation, the most glibc adds to one** ([`BLOCK_BYTES`]): the difference of the two files,
//! so that what both hold (the document, the page, the indexes) is not in it. The whole of each file is held to the same law too, and so is a file that holds one
//! of everything (the fuzz seed).
//!
//! Each count is one more than a power of two: a vector that grows by doubling then has room for twice what it holds, the most a read leaves it. The count is
//! 16,385 where the model's caps allow it, and under them where they do not (4,096 cells in a sheet, 8,192 sections, 4,096 colours, 1,000 pages, 256 themes).
//! A kind whose units are equal would be shared by the reader, so each unit is its own unless the kind is about what is shared.

#[cfg(target_os = "linux")]
use std::process::Command;

use diacraft_testkit::package::{CT_MASTER, CT_MASTERS, MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::{Limits, read_partial};
use stats_alloc::Region;

/// The kind and the count a child reads, as `kind:n`.
const KIND: &str = "DIACRAFT_VSDX_COUNTED_KIND";

/// The most the system's allocator (glibc's) adds to an allocation, in bytes: a request of `n` bytes is a chunk of `n` and 8, rounded up to 16 and never under 32
/// (`diacraft_sheetxml::model::cost::chunk`), so at most 24 bytes more for a request of 8 bytes or more (the figure). A large allocation is mapped on
/// its own and rounded up to a page, at most 4 KiB once, which the charges of the thousands of units it holds cover.
#[cfg(target_os = "linux")]
pub(crate) const BLOCK_BYTES: u64 = 24;

/// The share of the bytes kept that the allocator of the shipped app and command line (mimalloc) may add by rounding a request up to one of its size classes, as
/// the denominator of a fraction: an eighth. mimalloc has no header, but it rounds a request to one of four classes for each doubling; its
/// usable size was counted at up to 10.7 percent over the bytes asked for (a geometry section of one cell), and glibc's chunk is the
/// [`BLOCK_BYTES`] beside it.
#[cfg(target_os = "linux")]
pub(crate) const CLASS_SHARE: u64 = 8;

fn many(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(f).collect()
}

/// A text of `len` bytes of the `i`th unit's own: its number, then letters (nothing XML or a formula's string would escape). The kinds made mostly of long text
/// hold it: a cell's formula of 10 KiB that is one string, a shape's text of one run of 64 KiB, a kept element whose text is 1 MiB.
fn long_text(len: usize, i: usize) -> String {
    let head = format!("u{i}x");
    let fill = len.saturating_sub(head.len());
    head + &"a".repeat(fill)
}

/// A drawing of one page that holds `shapes`.
fn page_of(shapes: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shapes)).build()
}

/// A drawing of one page whose one shape holds `inner`.
fn one_shape(inner: &str) -> Vec<u8> {
    page_of(&shape(1, inner))
}

/// A masters index of `n` rows that all name one master part of one shape, with the relationships part that says so.
fn masters_index(n: usize) -> Vec<u8> {
    masters_index_of(n, |i| format!("<Master ID=\"{i}\" NameU=\"M{i}\" Name=\"M{i}\" UniqueID=\"{{GUID-{i}}}\"><Rel r:id=\"rId1\"/></Master>"))
}

/// A masters index of `n` rows made by `row`, which all name one master part of one shape (`rId1`).
fn masters_index_of(n: usize, row: impl Fn(usize) -> String) -> Vec<u8> {
    let rows = many(n, row);
    let masters = format!("<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Masters>");
    let master = format!("<MasterContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></MasterContents>", shape(4, ""));
    let pkg = Pkg::new()
        .part("/visio/masters/masters.xml", CT_MASTERS, masters)
        .rel("/visio/document.xml", "rId60", &format!("{VIS}masters"), "masters/masters.xml")
        .part("/visio/masters/master1.xml", CT_MASTER, master)
        .rel("/visio/masters/masters.xml", "rId1", &format!("{VIS}master"), "master1.xml");
    VsdxBuilder::new().without_masters_part().with(pkg).page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build()
}

/// A shape that shows the picture of relationship `rId{k}`.
fn picture_shape(k: usize) -> String {
    format!("<Shape ID=\"{k}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId{k}\"/></ForeignData></Shape>")
}

/// The custom properties part of `n` properties, and the package's relationship to it.
fn custom_props(n: usize) -> Pkg {
    let props = many(n, |i| {
        format!("<property fmtid=\"{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}\" pid=\"{}\" name=\"N{i}\"><vt:lpwstr>v{i}</vt:lpwstr></property>", i + 2)
    });
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">{props}</Properties>"
    );
    Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", xml).rel(
        "/",
        "rId9",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    )
}

/// The file of `n` units of `kind`. A kind and its count of nothing is the file the units are measured against.
pub(crate) fn file_of(kind: &str, n: usize) -> Vec<u8> {
    match kind {
        // Shapes with nothing in them share one empty sheet: what a shape costs on its own.
        "shape" => page_of(&many(n, |i| format!("<Shape ID=\"{i}\"/>"))),
        "shape_named" => page_of(&many(n, |i| shape_with(i as u32, &format!(" Type=\"Shape\" Name=\"Box{i}\" NameU=\"Box{i}\""), ""))),
        // A shape whose sheet holds one cell of its own: the sheet, and the map of its cells at its first leaf.
        "shape_cell" => page_of(&many(n, |i| shape(i as u32, &cell("PinX", &i.to_string())))),
        "shape_attrs" => page_of(&many(n, |i| format!("<Shape ID=\"{i}\" zz=\"{i}\" yy=\"y\"/>"))),
        "shape_data" => page_of(&many(n, |i| shape(i as u32, &format!("<Data1>datum {i}</Data1>")))),
        "group" => page_of(&many(n, |i| {
            format!("<Shape ID=\"{}\" Type=\"Group\"><Shapes><Shape ID=\"{}\"/></Shapes></Shape>", 2 * i + 1, 2 * i + 2)
        })),
        "instance" => {
            let master = MasterSpec::new(1, "M").shapes(&shape(1, &cell("Width", "1")));
            VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&many(n, |i| shape_with(i as u32 + 100, " Master=\"1\"", "")))).build()
        }
        "cell" => one_shape(&many(n, |i| cell(&format!("C{i}"), &i.to_string()))),
        "cell_listed" => {
            // Cells the catalogue names, a value each: no allocation for the name. Several shapes, each its own (a value of its own).
            let names = ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX"];
            page_of(&many(n, |i| shape(i as u32, &names.iter().map(|c| cell(c, &i.to_string())).collect::<String>())))
        }
        "cell_valued" => one_shape(&many(n, |i| format!("<Cell N=\"C{i}\" V=\"{i}\" U=\"IN\" F=\"Width*{i}\"/>"))),
        "cell_formula" => one_shape(&many(n, |i| format!("<Cell N=\"C{i}\" V=\"1\" F=\"(Width+Height)*(PinX-PinY)+{i}\"/>"))),
        "cell_attrs" => one_shape(&many(n, |i| format!("<Cell N=\"C{i}\" V=\"{i}\" a=\"aaaa\" b=\"bbbbbbbb\" c=\"c\"/>"))),
        "cell_text" => one_shape(&many(n, |i| format!("<Cell N=\"C{i}\" V=\"1\">some text of the cell {i}</Cell>"))),
        "cell_markup" => one_shape(&many(n, |i| format!("<Cell N=\"C{i}\" V=\"1\"><RefBy ID=\"{i}\" Del=\"1\"/><?NewValue V?></Cell>"))),
        "row" => one_shape(&format!("<Section N=\"User\">{}</Section>", many(n, |i| format!("<Row N=\"r{i}\"/>")))),
        "row_cells" => one_shape(&format!(
            "<Section N=\"Geometry\" IX=\"0\">{}</Section>",
            many(n, |i| format!("<Row T=\"LineTo\" IX=\"{i}\"><Cell N=\"X\" V=\"{i}\" U=\"IN\"/><Cell N=\"Y\" V=\"1\"/></Row>"))
        )),
        "section" => one_shape(&many(n, |i| format!("<Section N=\"S{i}\"/>"))),
        "section_geometry" => one_shape(&many(n, |i| format!("<Section N=\"Geometry\" IX=\"{i}\"><Cell N=\"NoFill\" V=\"{i}\"/></Section>"))),
        "connect" => VsdxBuilder::new()
            .page(PageSpec::new(1, "P").shapes(&shape(1, "")).connects(&many(n, |i| {
                format!("<Connect FromSheet=\"{i}\" FromCell=\"BeginX\" FromPart=\"9\" ToSheet=\"1\" ToCell=\"PinX\" ToPart=\"3\"/>")
            })))
            .build(),
        "connect_kept" => VsdxBuilder::new()
            .page(PageSpec::new(1, "P").shapes(&shape(1, "")).connects(&many(n, |i| {
                format!("<Connect FromSheet=\"0{i}\" FromCell=\"BeginX\" ToSheet=\"1\" ToCell=\"PinX\" k=\"v{i}\"/>")
            })))
            .build(),
        "text" => page_of(&many(n, |i| shape(i as u32, &format!("<Text><cp IX=\"0\"/>Hello world {i}</Text>")))),
        "text_parts" => one_shape(&format!(
            "<Text>{}</Text>",
            many(n, |i| match i % 4 {
                0 => format!("<cp IX=\"{i}\"/>"),
                1 => format!("words {i} "),
                2 => format!("<fld IX=\"{i}\">F{i}</fld>"),
                _ => format!("<pp IX=\"{i}\"/>"),
            })
        )),
        "unknown" => one_shape(&many(n, |_| "<z/>".to_owned())),
        "unknown_tree" => one_shape(&many(n, |i| format!("<zz a=\"{i}\"><y>text</y></zz>"))),
        "document_unknown" => VsdxBuilder::new().document_extra(&many(n, |_| "<z/>".to_owned())).build(),
        "style" => VsdxBuilder::new()
            .more_styles(&many(n, |i| {
                format!(
                    "<StyleSheet ID=\"{}\" Name=\"S{i}\" NameU=\"S{i}\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Cell N=\"LineWeight\" V=\"0.0{i}\"/></StyleSheet>",
                    i + 1
                )
            }))
            .build(),
        "master" => masters_index(n),
        "master_part" => {
            let mut b = VsdxBuilder::new();
            for i in 0..n {
                b = b.master(MasterSpec::new(i as u32 + 1, &format!("M{i}")).shapes(&shape(1, &cell("Width", &i.to_string()))));
            }
            b.build()
        }
        "page" => {
            let mut b = VsdxBuilder::new();
            for i in 0..n {
                b = b.page(PageSpec::new(i as u32 + 1, &format!("P{i}")).shapes(&shape(1, &cell("Width", &i.to_string()))));
            }
            b.build()
        }
        "page_row" => {
            let mut b = VsdxBuilder::new();
            for i in 0..n {
                b = b.page(PageSpec::new(i as u32 + 1, &format!("P{i}")).without_part());
            }
            b.build()
        }
        "pages_sharing_part" => super::timing::pages_sharing_one_part(n, 4),
        // One of everything a file can hold: the units are the whole file.
        "seed" => super::fuzz::seed_package().build(),
        "color" => VsdxBuilder::new().colors(&many(n, |i| format!("<ColorEntry IX=\"{}\" RGB=\"#{:06X}\"/>", i + 24, i))).build(),
        "color_kept" => VsdxBuilder::new().colors(&many(n, |i| format!("<ColorEntry IX=\"{}\" RGB=\"#{:06X}\" Z=\"{i}\"/>", i + 24, i))).build(),
        "face" => VsdxBuilder::new().face_names(&many(n, |i| format!("<FaceName NameU=\"Face {i}\" Panose=\"{i}\"/>"))).build(),
        // Shapes that show a picture the package does not have: the foreign data, and no picture.
        "foreign" => page_of(&many(n, |i| {
            format!("<Shape ID=\"{i}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\" CompressionType=\"PNG\"><Rel r:id=\"rId{i}\"/></ForeignData></Shape>")
        })),
        "picture" => {
            let mut page = PageSpec::new(1, "P").shapes(&many(n, |k| picture_shape(k + 1)));
            let mut pkg = Pkg::new();
            for k in 1..=n {
                page = page.rel(&format!("rId{k}"), &format!("{OD}image"), &format!("../media/image{k}.png"));
                pkg = pkg.part(&format!("/visio/media/image{k}.png"), "image/png", format!("picture {k}").into_bytes());
            }
            VsdxBuilder::new().page(page).with(pkg).build()
        }
        // Parts that no field of the model rebuilds: held as parts of the document, with their bytes.
        "held_part" => {
            let mut pkg = Pkg::new();
            for k in 0..n {
                pkg = pkg.part(&format!("/custom/item{k}.bin"), "application/octet-stream", format!("part {k}").into_bytes());
            }
            VsdxBuilder::new().with(pkg).build()
        }
        // Relationships of a page that no field of the model says: held rows.
        "held_rel" => {
            let mut page = PageSpec::new(1, "P").shapes(&shape(1, ""));
            for k in 0..n {
                page = page.rel(&format!("rId{}", k + 10), "urn:filler", &format!("x{k}.bin"));
            }
            VsdxBuilder::new().page(page).build()
        }
        // `Default` rows for extensions no part has: held rows.
        "content_type" => {
            let mut pkg = Pkg::new();
            for k in 0..n {
                pkg = pkg.default_type(&format!("e{k}"), &format!("application/x-{k}"));
            }
            VsdxBuilder::new().with(pkg).build()
        }
        "custom_prop" => VsdxBuilder::new().with(custom_props(n)).build(),
        "master_icon" => masters_index_of(n, |i| format!("<Master ID=\"{i}\" NameU=\"M{i}\"><Rel r:id=\"rId1\"/><Icon>AAAA{i}</Icon></Master>")),
        // A `Rel` that holds an attribute besides its id: held in the master's slot.
        "master_rel_kept" => masters_index_of(n, |i| format!("<Master ID=\"{i}\" NameU=\"M{i}\"><Rel r:id=\"rId1\" zz=\"{i}\"/></Master>")),
        "page_row_kept" => {
            let mut b = VsdxBuilder::new();
            for i in 0..n {
                b = b.page(PageSpec::new(i as u32 + 1, &format!("P{i}")).attrs(&format!(" zz=\"{i}\"")).without_part());
            }
            b.build()
        }
        // A page part whose root has an attribute no field reads: held in the part's list, and in the slot of the page that names it.
        "page_root_kept" => {
            let mut b = VsdxBuilder::new();
            for i in 0..n {
                let part = format!("<PageContents xmlns=\"{NS_2012}\" zz=\"{i}\"><Shapes>{}</Shapes></PageContents>", shape(1, &cell("Width", &i.to_string())));
                b = b.page(PageSpec::new(i as u32 + 1, &format!("P{i}")).part(&part));
            }
            b.build()
        }
        "section_kept" => one_shape(&many(n, |i| format!("<Section N=\"Geometry\" IX=\"{i}\" Z=\"zz{i}\"/>"))),
        "row_kept" => one_shape(&format!(
            "<Section N=\"Geometry\" IX=\"0\">{}</Section>",
            many(n, |i| format!("<Row T=\"LineTo\" IX=\"{i}\" LocalName=\"l{i}\" q=\"{i}\"/>"))
        )),
        // A text that holds an element that is no marker: kept whole.
        "text_kept" => page_of(&many(n, |i| shape(i as u32, &format!("<Text>x<b>{i}</b></Text>")))),
        // Kept nodes in a shape's element and in its sheet's, one between the other's: the order they came in is held.
        "shape_order" => page_of(&many(n, |i| shape(i as u32, &format!("<zz/>{}<yy/>", cell("PinX", &i.to_string()))))),
        "style_kept" => VsdxBuilder::new()
            .more_styles(&many(n, |i| format!("<StyleSheet ID=\"{}\" NameU=\"S{i}\"><zz/><Cell N=\"LineWeight\" V=\"0.0{i}\"/><yy/></StyleSheet>", i + 1)))
            .build(),
        "windows" => {
            let windows = many(n, |i| format!("<Window ID=\"{i}\" WindowType=\"Drawing\" ViewScale=\"1\"/>"));
            let xml = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?><Windows xmlns=\"{NS_2012}\">{windows}</Windows>");
            let pkg = Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", xml).rel(
                "/visio/document.xml",
                "rId70",
                &format!("{VIS}windows"),
                "windows.xml",
            );
            VsdxBuilder::new().with(pkg).build()
        }
        // The elements of `docProps/app.xml` that no field reads: held.
        "app_kept" => {
            let rest = many(n, |i| format!("<Extra{}>v{i}</Extra{}>", i % 7, i % 7));
            VsdxBuilder::new()
                .app(&format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Test Writer</Application>{rest}</Properties>"
                ))
                .build()
        }
        // Kinds made mostly of long text: a formula's tree holds its string again beside the source.
        "formula_long" => one_shape(&many(n, |i| format!("<Cell N=\"C{i}\" V=\"1\" F=\"&quot;{}&quot;\"/>", long_text(10 * 1024 - 2, i)))),
        "text_long" => page_of(&many(n, |i| shape(i as u32, &format!("<Text>{}</Text>", long_text(64 * 1024, i))))),
        "kept_long" => page_of(&many(n, |i| shape(i as u32, &format!("<zz>{}</zz>", long_text(1024 * 1024, i))))),
        "theme" => {
            let mut b = VsdxBuilder::new();
            for k in 0..n {
                let theme = format!("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"T{k}\"><a:themeElements/></a:theme>");
                b = b.theme(theme.as_bytes());
            }
            b.build()
        }
        other => panic!("no kind {other}"),
    }
}

/// What a child counted of one read.
#[derive(Clone, Copy, Debug)]
#[cfg(target_os = "linux")]
struct Counted {
    /// The bytes the document holds, as the allocator was asked for them, and the allocations that hold them.
    kept: u64,
    blocks: u64,
    /// What the read charged: the pool's charge when it was done, which holds the held parts' bytes.
    charged: u64,
}

#[cfg(target_os = "linux")]
impl Counted {
    /// **The law** (widened by to the allocator the app ships with): what the allocators give the document, its bytes, an eighth of them
    /// for mimalloc's size classes and, for each allocation, the most glibc adds to one.
    fn need(&self) -> u64 {
        self.kept.saturating_add(self.kept / CLASS_SHARE).saturating_add(self.blocks.saturating_mul(BLOCK_BYTES))
    }
}

/// Reads `bytes` and counts what the document keeps.
fn count_read(bytes: &[u8]) -> (bool, i128, i128, u64) {
    let signed = |n: usize| i128::try_from(n).unwrap_or(i128::MAX);
    let region = Region::new(crate::GLOBAL);
    let (result, report) = read_partial(bytes, &Limits::default());
    let charged = report.stats.memory_bytes;
    drop(report);
    let change = region.change();
    // A reallocation's growth is in the bytes allocated and its shrinking in the bytes given back (`bytes_reallocated` is the two again, as one sum).
    let kept = signed(change.bytes_allocated) - signed(change.bytes_deallocated);
    let blocks = signed(change.allocations) - signed(change.deallocations);
    let ok = result.is_ok();
    drop(result);
    (ok, kept, blocks, charged)
}

#[test]
#[ignore = "run by the tests below in a process of its own; alone it needs the kind in the environment"]
fn counted_child() {
    let Ok(spec) = std::env::var(KIND) else { return };
    let (kind, n) = spec.split_once(':').expect("a kind and a count");
    let n: usize = n.parse().expect("a count");
    // The code a read runs, and what it makes once for the process (a table, a catalogue), is made by a small read first.
    let _ = count_read(&file_of(kind, n.min(3)));
    let probe = Region::new(crate::GLOBAL);
    drop(std::hint::black_box(Vec::<u8>::with_capacity(4096)));
    let counting = probe.change().bytes_allocated >= 4096;
    for (label, units) in [("base", 0), ("full", n)] {
        let bytes = file_of(kind, units);
        let (ok, kept, blocks, charged) = count_read(&bytes);
        println!("COUNTED {label} ok={ok} counting={counting} n={units} kept={kept} blocks={blocks} charged={charged}");
    }
}

/// Writes the files of the kinds named in `DIACRAFT_VSDX_COUNTED_SAVE` (`kind:n,...`) to the folder `DIACRAFT_VSDX_COUNTED_DIR`, each as `<kind>-0.vsdx` and
/// `<kind>-<n>.vsdx`, so that a read of them can be counted by hand under another allocator (the report of counts them under mimalloc, the allocator
/// of the app and the command line).
#[test]
#[ignore = "a probe, run by hand"]
fn save_counted_inputs() {
    let (Ok(specs), Ok(dir)) = (std::env::var("DIACRAFT_VSDX_COUNTED_SAVE"), std::env::var("DIACRAFT_VSDX_COUNTED_DIR")) else { return };
    for spec in specs.split(',') {
        let (kind, n) = spec.split_once(':').expect("a kind and a count");
        let n: usize = n.parse().expect("a count");
        for units in [0, n] {
            std::fs::write(std::path::Path::new(&dir).join(format!("{kind}-{units}.vsdx")), file_of(kind, units)).expect("the input written");
        }
    }
}

/// The reads of a kind with none and with `n` units, as the child counted them.
#[cfg(target_os = "linux")]
fn measure(kind: &str, n: usize) -> (Counted, Counted) {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args(["--exact", "memory_counted::counted_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(KIND, format!("{kind}:{n}"))
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line = |label: &str| -> Counted {
        let key = format!("COUNTED {label} ");
        let line = text.find(&key).map(|at| text[at..].lines().next().unwrap_or_default()).unwrap_or_else(|| panic!("{kind}: no count in {text}"));
        assert!(line.contains("ok=true"), "{kind}: the read failed: {line}");
        assert!(line.contains("counting=true"), "{kind}: the binary's allocator does not count what is allocated: {line}");
        let field = |key: &str| -> u64 {
            let value: i128 = line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number");
            u64::try_from(value).unwrap_or_else(|_| panic!("{kind}: {key} is not a count: {line}"))
        };
        Counted { kept: field("kept="), blocks: field("blocks="), charged: field("charged=") }
    };
    (line("base"), line("full"))
}

/// The units of `kind` at `n`: **their charge covers what they keep**, and so does the charge of the whole file.
#[cfg(target_os = "linux")]
fn check(kind: &str, n: usize) {
    let (base, full) = measure(kind, n);
    let units = Counted {
        kept: full.kept.saturating_sub(base.kept),
        blocks: full.blocks.saturating_sub(base.blocks),
        charged: full.charged.saturating_sub(base.charged),
    };
    let per = |x: u64| x as f64 / n as f64;
    eprintln!(
        "{kind:<20} {n:>6} units: kept {:>8.1} bytes in {:>5.2} allocations each ({:>8.1} with the allocators'), charged {:>8.1}: {:.3} of the need; the whole file {:.3}",
        per(units.kept),
        per(units.blocks),
        per(units.need()),
        per(units.charged),
        units.charged as f64 / units.need().max(1) as f64,
        full.charged as f64 / full.need().max(1) as f64,
    );
    let mut problems = Vec::new();
    if units.charged < units.need() {
        problems.push(format!(
            "{kind}: {n} units keep {} bytes in {} allocations ({} with what the allocators add) and were charged {}",
            units.kept,
            units.blocks,
            units.need(),
            units.charged
        ));
    }
    if full.charged < full.need() {
        problems.push(format!(
            "{kind}: the whole file keeps {} bytes in {} allocations ({} with what the allocators add) and was charged {}",
            full.kept,
            full.blocks,
            full.need(),
            full.charged
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The whole of `kind`'s file: its charge covers what its document keeps.
#[cfg(target_os = "linux")]
fn check_whole(kind: &str) {
    let (_, full) = measure(kind, 0);
    eprintln!(
        "{kind:<20} the whole file: kept {} bytes in {} allocations ({} with the allocators'), charged {}: {:.3} of the need",
        full.kept,
        full.blocks,
        full.need(),
        full.charged,
        full.charged as f64 / full.need().max(1) as f64
    );
    assert!(
        full.charged >= full.need(),
        "{kind}: the whole file keeps {} bytes in {} allocations and was charged {}",
        full.kept,
        full.blocks,
        full.charged
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_file_of_one_of_everything_is_charged_what_it_keeps() {
    check_whole("seed");
}

#[test]
#[cfg(target_os = "linux")]
fn a_file_of_nothing_is_charged_what_it_keeps() {
    check_whole("shape");
}

macro_rules! kinds {
    ($($test:ident: $kind:literal, $count:expr;)*) => {
        $(
            #[test]
            #[cfg(target_os = "linux")]
            fn $test() {
                check($kind, $count);
            }
        )*
    };
}

kinds! {
    a_shape_is_charged_what_it_keeps: "shape", 16_385;
    a_shape_with_names_is_charged_what_it_keeps: "shape_named", 16_385;
    a_shape_with_a_cell_of_its_own_is_charged_what_it_keeps: "shape_cell", 16_385;
    a_shape_that_keeps_attributes_is_charged_what_it_keeps: "shape_attrs", 16_385;
    a_shape_with_a_datum_is_charged_what_it_keeps: "shape_data", 16_385;
    a_group_of_one_shape_is_charged_what_it_keeps: "group", 16_385;
    an_instance_of_a_master_is_charged_what_it_keeps: "instance", 16_385;
    a_cell_is_charged_what_it_keeps: "cell", 2_049;
    cells_the_catalogue_names_are_charged_what_they_keep: "cell_listed", 16_385;
    a_cell_with_a_value_a_unit_and_a_formula_is_charged_what_it_keeps: "cell_valued", 2_049;
    a_cell_with_a_long_formula_is_charged_what_it_keeps: "cell_formula", 2_049;
    a_cell_that_keeps_attributes_is_charged_what_it_keeps: "cell_attrs", 2_049;
    a_cell_that_holds_text_is_charged_what_it_keeps: "cell_text", 2_049;
    a_cell_that_holds_markup_is_charged_what_it_keeps: "cell_markup", 2_049;
    a_row_is_charged_what_it_keeps: "row", 16_385;
    a_row_of_two_cells_is_charged_what_it_keeps: "row_cells", 16_385;
    a_section_is_charged_what_it_keeps: "section", 4_097;
    a_geometry_section_is_charged_what_it_keeps: "section_geometry", 4_097;
    a_connect_is_charged_what_it_keeps: "connect", 16_385;
    a_connect_that_keeps_attributes_is_charged_what_it_keeps: "connect_kept", 16_385;
    a_text_is_charged_what_it_keeps: "text", 16_385;
    the_parts_of_a_text_are_charged_what_they_keep: "text_parts", 16_385;
    an_unknown_element_is_charged_what_it_keeps: "unknown", 16_385;
    an_unknown_tree_is_charged_what_it_keeps: "unknown_tree", 16_385;
    an_unknown_element_of_the_document_is_charged_what_it_keeps: "document_unknown", 16_385;
    a_style_is_charged_what_it_keeps: "style", 16_385;
    a_master_row_is_charged_what_it_keeps: "master", 16_385;
    a_master_with_a_part_of_its_own_is_charged_what_it_keeps: "master_part", 513;
    a_page_is_charged_what_it_keeps: "page", 513;
    a_page_row_with_no_part_is_charged_what_it_keeps: "page_row", 513;
    pages_that_share_a_part_are_charged_what_they_keep: "pages_sharing_part", 513;
    a_colour_is_charged_what_it_keeps: "color", 2_049;
    a_colour_that_keeps_attributes_is_charged_what_it_keeps: "color_kept", 2_049;
    a_face_is_charged_what_it_keeps: "face", 16_385;
    a_shape_that_shows_a_picture_the_package_does_not_have_is_charged_what_it_keeps: "foreign", 16_385;
    a_picture_is_charged_what_it_keeps: "picture", 2_049;
    a_held_part_is_charged_what_it_keeps: "held_part", 16_385;
    a_held_relationship_is_charged_what_it_keeps: "held_rel", 16_385;
    a_held_content_type_row_is_charged_what_it_keeps: "content_type", 16_385;
    a_custom_property_is_charged_what_it_keeps: "custom_prop", 16_385;
    a_theme_is_charged_what_it_keeps: "theme", 129;
    a_master_icon_is_charged_what_it_keeps: "master_icon", 16_385;
    a_master_relationship_that_keeps_attributes_is_charged_what_it_keeps: "master_rel_kept", 16_385;
    a_page_row_that_keeps_attributes_is_charged_what_it_keeps: "page_row_kept", 513;
    a_page_part_whose_root_keeps_attributes_is_charged_what_it_keeps: "page_root_kept", 513;
    a_section_that_keeps_attributes_is_charged_what_it_keeps: "section_kept", 4_097;
    a_row_that_keeps_a_type_a_local_name_and_attributes_is_charged_what_it_keeps: "row_kept", 16_385;
    a_text_that_keeps_an_element_is_charged_what_it_keeps: "text_kept", 16_385;
    the_order_of_kept_nodes_is_charged_what_it_keeps: "shape_order", 16_385;
    a_style_that_keeps_nodes_is_charged_what_it_keeps: "style_kept", 16_385;
    a_window_is_charged_what_it_keeps: "windows", 16_385;
    an_element_of_the_application_properties_is_charged_what_it_keeps: "app_kept", 16_385;
    a_cell_of_a_long_formula_is_charged_what_it_keeps: "formula_long", 2_049;
    a_long_text_run_is_charged_what_it_keeps: "text_long", 257;
    a_long_kept_text_is_charged_what_it_keeps: "kept_long", 17;
}
