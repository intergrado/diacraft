//! **What the `.odg` reader admits, saved as a `.diacraft` and opened again.** A drawing is read by the `.odg` reader, saved a part
//! at a time as `.diacraft` parts (`diacraft_sheetxml::model::save_parts`, at the limits of an open), let go, and opened a part at a time from the bytes of the parts, in
//! a child under `ulimit -v 3000000`; the child prints what it measured on a `NATIVE` line. A save that the open would refuse ends refused in words; an abort is the
//! failure. The cases are the reader's own memory cases: its 350,000 rectangles, rectangles that each have a size of their own, and paths of 46 commands.
use std::process::Command;

use diacraft_container::Limits as ContainerLimits;
use diacraft_container::xml::{El, parse_with, to_bytes};
use diacraft_odg::{Limits, read};
use diacraft_sheetxml::model::{Budget, CodecError, MemBlobs, PartRole, PartSource, decode_from, open_limits, save_parts, save_parts_bytes};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::read_hostile::unshared_rects;

const SCENARIO: &str = "ODG_NATIVE_CAPACITY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// A flat document of `n` rectangles that share a size (the reader shares what it builds for them), as `fodg_memory.rs` makes it.
fn rects(n: usize) -> Vec<u8> {
    let body: String = (1..=n)
        .map(|k| {
            format!(
                "\n    <draw:rect draw:name=\"s{k}\" svg:x=\"{}.{}cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"/>",
                k % 997,
                k % 89 + 1
            )
        })
        .collect();
    Flat::of(&Drawing::a4().page(&body)).bytes()
}

/// A package of `n` paths of 46 commands each (the reader's path-heavy case, `memory_path_heavy_drawing`).
fn paths(n: usize) -> Vec<u8> {
    let path = |k: usize| {
        let d: String = (0..45).map(|j| format!(" L{} {}", (j * 7 + k) % 1000, (j * 13) % 1000)).collect();
        format!(r#"<draw:path svg:x="{}mm" svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 1000 1000" svg:d="M0 0{d}"/>"#, k % 200)
    };
    let shapes: String = (0..n).map(path).collect();
    Drawing::a4().page(&shapes).bytes()
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
    let (kind, n) = spec.split_once(':').map(|(k, n)| (k.to_owned(), n.parse::<usize>().unwrap_or(1000))).expect("kind:count");
    let bytes = match kind.as_str() {
        "rects" => rects(n),
        "unshared" => Drawing::a4().page(&unshared_rects(n)).bytes(),
        "paths" => paths(n),
        other => panic!("no kind {other}"),
    };
    let (doc, report) = match read(&bytes, &Limits::default()) {
        Ok(read) => read,
        Err(e) => {
            println!("NATIVE {spec} read={} VmPeak {} MiB", e.class(), status_kib("VmPeak") >> 10);
            println!("native capacity child: no abort");
            return;
        }
    };
    drop((bytes, report));
    let read_peak = status_kib("VmPeak") >> 10;
    let limits = open_limits();
    let held_mib = diacraft_sheetxml::model::held_bytes(&doc) >> 20;
    let stream = std::env::var("NATIVE_TREE").is_err();
    let (mut blobs, mut document, mut pages, mut written) = (MemBlobs::new(), Vec::new(), Vec::new(), 0u64);
    let saved = if stream {
        save_parts_bytes(&doc, &mut blobs, &limits, &mut |part| {
            written += part.bytes.len() as u64;
            match part.role {
                PartRole::Document => document = part.bytes.to_vec(),
                PartRole::Page { .. } => pages.push(part.bytes.to_vec()),
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
                PartRole::Page { .. } => pages.push(bytes),
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
                &mut FromBytes { pages: &pages, limits: &limits, stream },
                &blobs,
                &mut budget,
                &mut Vec::new(),
            );
            let charged =
                (diacraft_sheetxml::model::OPEN_MODEL_BYTES - budget.bytes) + (diacraft_sheetxml::model::OPEN_BLOB_BYTES - budget.blob_bytes);
            format!(
                "saved {} MiB in {} parts (largest page part {} items, largest child {}, document part {}), model held {held_mib} MiB and charged {} MiB, opened: {}",
                written >> 20,
                1 + pages.len(),
                pages.iter().map(|b| part_items(b).0).max().unwrap_or(0),
                pages.iter().map(|b| part_items(b).1).max().unwrap_or(0),
                part_items(&document).0,
                charged >> 20,
                opened.map_or_else(|e| e.to_string(), |_| "yes".to_owned())
            )
        }
        Err(e) => format!("refused in words (model held {held_mib} MiB): {e}"),
    };
    println!(
        "NATIVE {spec} shapes={shapes} read_peak={read_peak} MiB save_peak={save_peak} MiB final_peak={} MiB: {outcome}",
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
    eprintln!("{}", line.clone().unwrap_or_else(|| format!("{spec}: the child ended with {:?}: {text}", out.status)));
    assert!(out.status.success() && text.contains("no abort"), "{spec}: the child aborted or failed: {text}");
    line.unwrap_or_default()
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn ten_thousand_rectangles_of_the_odg_reader_are_saved_as_a_diacraft_and_opened_under_the_address_limit() {
    let line = child("rects:10000");
    assert!(line.contains("opened: yes"), "{line}");
}

/// The measurements the report states: `ODG_NATIVE_SIZES=rects:48000,rects:350000 cargo test -- --ignored native_numbers --nocapture`.
#[test]
#[ignore = "a measurement: it prints what each size of drawing is saved and opened as"]
fn native_numbers() {
    let sizes = std::env::var("ODG_NATIVE_SIZES").unwrap_or_else(|_| "rects:48000,rects:350000,unshared:30000,paths:35191".to_owned());
    for spec in sizes.split(',') {
        let _ = child(spec);
    }
}
