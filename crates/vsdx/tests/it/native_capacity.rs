//! **What the `.vsdx` reader admits, saved as a `.diacraft` and opened again.** A package of shapes of the kind a diagram has is
//! built by the testkit, read by the `.vsdx` reader, saved a part at a time as `.diacraft` parts (`diacraft_sheetxml::model::save_parts`, at the limits of an open),
//! let go, and opened a part at a time from the bytes of the parts, in a child under `ulimit -v 3000000`; the child prints what it measured on a `NATIVE` line.
//!
//! **What this reader admits today is not what 3 GB holds** (will lower its caps: 20 million items, 16 million cells, 2 million shapes): a package that
//! the reader admits may not be read under the address limit at all, and the line says so, as the reader's own `read=` result.
use std::process::Command;

use diacraft_container::Limits as ContainerLimits;
use diacraft_container::xml::{El, parse_with, to_bytes};
use diacraft_sheetxml::model::{Budget, CodecError, MemBlobs, PartRole, PartSource, decode_from, open_limits, save_parts, save_parts_bytes};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use diacraft_vsdx::{Limits, read};

const SCENARIO: &str = "VSDX_NATIVE_CAPACITY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// A shape of the kind a diagram has, as a file of the format spells it: fourteen cells (two with formulas), a path of five rows of two cells, and a text.
fn shape(i: usize) -> String {
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

struct FromBytes<'a> {
    pages: &'a [Vec<u8>],
    limits: &'a ContainerLimits,
    /// Whether the parts are read as a stream of the children of their root (a source that holds a part as bytes: the default), or as the tree of each (`NATIVE_TREE=1`, which
    /// is what the save and the open were before a change, for the figures of the report that say "before").
    stream: bool,
}

impl PartSource for FromBytes<'_> {
    type Tree = El;

    fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        let bytes = self.pages.get(index).ok_or_else(|| CodecError::Malformed { part: "page".to_owned(), why: "no such part" })?;
        parse_with(bytes, &self.limits.xml).map_err(|_| CodecError::Malformed { part: "page".to_owned(), why: "a part the reader refuses" })
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "master".to_owned(), why: "no such part" })
    }

    fn page_bytes(&mut self, index: usize, _name: &str) -> Result<Option<std::borrow::Cow<'_, [u8]>>, CodecError> {
        Ok(if self.stream { self.pages.get(index).map(|b| std::borrow::Cow::Borrowed(b.as_slice())) } else { None })
    }
}

/// The items of a part's bytes and of its largest child of the root, as a reader that reads a part as a stream counts them (the largest tree held at once is a child's).
fn part_items(bytes: &[u8]) -> (u64, u64) {
    use diacraft_container::XmlLimits;
    use diacraft_container::xml::{Node, ParseOptions, PullLimits, XmlText};
    let limits = PullLimits { xml: XmlLimits { max_depth: 512, max_nodes: usize::MAX, ..XmlLimits::default() }, max_document_nodes: usize::MAX };
    let text = XmlText::new(bytes).expect("a part that is XML");
    let (mut pull, head) = text.pull(&limits, ParseOptions::default()).expect("a part with a root");
    let (mut total, mut largest) = (1 + head.attrs.len() as u64, 0u64);
    while let Some(child) = pull.next_child().expect("a part that is well formed") {
        let mut n = 0u64;
        let mut stack = vec![&child];
        while let Some(e) = stack.pop() {
            n += 1 + e.attrs.len() as u64;
            for c in &e.children {
                match c {
                    Node::El(c) => stack.push(c),
                    _ => n += 1,
                }
            }
        }
        total += n;
        largest = largest.max(n);
    }
    (total, largest)
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn native_child() {
    let spec = std::env::var(SCENARIO).unwrap_or_default();
    let (n, pages) = spec.split_once(':').map(|(n, p)| (n.parse::<usize>().unwrap_or(1000), p.parse::<usize>().unwrap_or(1))).expect("shapes:pages");
    // The package: one part for each page, the shapes of each made when the page is added and the text let go once it is in the archive.
    let per = n.div_ceil(pages);
    let mut builder = VsdxBuilder::new();
    for p in 0..pages {
        let body: String = (p * per..((p + 1) * per).min(n)).map(shape).collect();
        builder = builder.page(PageSpec::new(u32::try_from(p + 1).unwrap(), &format!("Page {}", p + 1)).shapes(&body));
    }
    let bytes = builder.build();
    drop(builder);
    let build_peak = status_kib("VmPeak") >> 10;
    let (doc, _report) = match read(&bytes, &Limits::default()) {
        Ok(read) => read,
        Err(e) => {
            println!(
                "NATIVE {spec} package={} MiB build_peak={build_peak} MiB read={} VmPeak {} MiB",
                bytes.len() >> 20,
                e.class(),
                status_kib("VmPeak") >> 10
            );
            println!("native capacity child: no abort");
            return;
        }
    };
    let package_mib = bytes.len() >> 20;
    drop((bytes, _report));
    let read_peak = status_kib("VmPeak") >> 10;
    println!("STAGE the reader admitted the package: {} shapes, VmPeak {read_peak} MiB", doc.pages.iter().map(|p| p.shapes.len()).sum::<usize>());
    let limits = open_limits();
    let held_mib = diacraft_sheetxml::model::held_bytes(&doc) >> 20;
    let stream = std::env::var("NATIVE_TREE").is_err();
    let (mut blobs, mut document, mut parts, mut written) = (MemBlobs::new(), Vec::new(), Vec::new(), 0u64);
    let saved = if stream {
        save_parts_bytes(&doc, &mut blobs, &limits, &mut |part| {
            written += part.bytes.len() as u64;
            match part.role {
                PartRole::Document => document = part.bytes.to_vec(),
                PartRole::Page { .. } => parts.push(part.bytes.to_vec()),
                PartRole::Master { .. } => {}
            }
            Ok(())
        })
    } else {
        save_parts(&doc, &mut blobs, &limits, &mut |part| {
            let bytes = to_bytes(part.tree, &[], &[], true).map_err(CodecError::from)?;
            written += bytes.len() as u64;
            match part.role {
                PartRole::Document => document = bytes,
                PartRole::Page { .. } => parts.push(bytes),
                PartRole::Master { .. } => {}
            }
            Ok(())
        })
    };
    let save_peak = status_kib("VmPeak") >> 10;
    let shapes = doc.pages.iter().map(|p| p.shapes.len()).sum::<usize>();
    let outcome = match &saved {
        Ok(()) => {
            drop(doc);
            let mut budget = Budget::for_open(&limits);
            let opened = decode_from(
                &parse_with(&document, &limits.xml).unwrap(),
                &mut FromBytes { pages: &parts, limits: &limits, stream },
                &blobs,
                &mut budget,
                &mut Vec::new(),
            );
            let charged =
                (diacraft_sheetxml::model::OPEN_MODEL_BYTES - budget.bytes) + (diacraft_sheetxml::model::OPEN_BLOB_BYTES - budget.blob_bytes);
            format!(
                "saved {} MiB in {} parts (largest page part {} items, largest child {}, document part {}), model held {held_mib} MiB and charged {} MiB, opened: {}",
                written >> 20,
                1 + parts.len(),
                parts.iter().map(|b| part_items(b).0).max().unwrap_or(0),
                parts.iter().map(|b| part_items(b).1).max().unwrap_or(0),
                part_items(&document).0,
                charged >> 20,
                opened.map_or_else(|e| e.to_string(), |_| "yes".to_owned())
            )
        }
        Err(e) => format!("refused in words (model held {held_mib} MiB): {e}"),
    };
    println!(
        "NATIVE {spec} shapes={shapes} package={package_mib} MiB build_peak={build_peak} MiB read_peak={read_peak} MiB save_peak={save_peak} MiB final_peak={} MiB: {outcome}",
        status_kib("VmPeak") >> 10
    );
    println!("native capacity child: no abort");
}

fn child(spec: &str) -> String {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact native_capacity::native_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, spec).output().expect("run /bin/sh");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line = text.lines().find_map(|l| l.find("NATIVE ").map(|at| l[at..].to_owned()));
    let stage = text
        .lines()
        .find_map(|l| l.find("STAGE ").map(|at| l[at..].to_owned()))
        .unwrap_or_else(|| "STAGE the reader did not admit the package".to_owned());
    eprintln!("{spec}: {stage}");
    eprintln!("{}", line.clone().unwrap_or_else(|| format!("{spec}: the child ended with {:?}", out.status)));
    assert!(out.status.success() && text.contains("no abort"), "{spec}: the child aborted or failed: {text}");
    line.unwrap_or_default()
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn ten_thousand_shapes_of_a_vsdx_are_saved_as_a_diacraft_and_opened_under_the_address_limit() {
    let line = child("10000:2");
    assert!(line.contains("opened: yes"), "{line}");
}

/// The measurements the report states: `VSDX_NATIVE_SIZES=90000:1,200000:4 cargo test -- --ignored native_numbers --nocapture`.
#[test]
#[ignore = "a measurement: it prints what each size of drawing is saved and opened as"]
fn native_numbers() {
    let sizes = std::env::var("VSDX_NATIVE_SIZES").unwrap_or_else(|_| "90000:1,100000:2,200000:4".to_owned());
    for spec in sizes.split(',') {
        let _ = child(spec);
    }
}
