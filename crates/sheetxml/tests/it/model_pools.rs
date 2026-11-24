//! The pools of the open budget and the one tree of the part being read fit under 3 GB **together**. The first
//! figures were sized one at a time (12 million items, a gibibyte of model, half a gibibyte of blobs), and a file that fills all three at once is 3.2 GB; the second
//! held every part's tree at once, which a decode that reads a part at a time ([`PartSource`]) does not.
//!
//! What the figures rest on is measured here: what a tree costs for each item at its worst, what a process holds before it opens anything, and then one decode with
//! every pool at its limit, in a process that is held to 3,000,000 KiB of address space.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, Node, XmlLimits, parse_with};
use diacraft_sheetxml::model::cost::{BASE_BYTES, TREE_ITEM_BYTES};
use diacraft_sheetxml::model::{
    ADDRESS_SPACE_BYTES, BlobId, BlobSource, Budget, CodecError, DecodeNote, OPEN_BLOB_BYTES, OPEN_DOCUMENT_FREE_ITEMS, OPEN_ELEMENT_ITEMS,
    OPEN_FILE_BYTES, OPEN_ITEMS, OPEN_MODEL_BYTES, OPEN_PACKAGE_BYTES, OPEN_PART_BYTES, OPEN_POOL_BYTES, OPEN_TREE_ITEMS, OPEN_WORST_BYTES,
    PartSource, SAVE_WORST_BYTES, SECTION_STREAM_BYTES, URN, decode_from, open_limits, stamp_of_bytes,
};

use super::model_util::native_limits;

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

// ------------------------------------------------------------------------------------------------------------------------------------- the limits

#[test]
fn the_pools_and_the_one_tree_at_their_worst_leave_a_twentieth_of_the_address_space() {
    let worst = OPEN_FILE_BYTES
        + OPEN_TREE_ITEMS * TREE_ITEM_BYTES
        + OPEN_DOCUMENT_FREE_ITEMS * TREE_ITEM_BYTES
        + 2 * OPEN_PART_BYTES
        + OPEN_BLOB_BYTES
        + OPEN_MODEL_BYTES
        + BASE_BYTES;
    assert_eq!(OPEN_WORST_BYTES, worst, "the sum the budget states is the sum of its terms");
    assert!(worst <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20, "{worst} of {ADDRESS_SPACE_BYTES} bytes");
    // Each is the whole of the figure the budget says, and not one that was left behind.
    assert_eq!(OPEN_ITEMS, 64_000_000);
    assert_eq!(OPEN_TREE_ITEMS, 2_000_000);
    assert_eq!(OPEN_DOCUMENT_FREE_ITEMS, 256_000);
    assert_eq!(OPEN_PART_BYTES, 128 << 20);
    assert_eq!(OPEN_MODEL_BYTES, 960 << 20);
    assert_eq!(OPEN_BLOB_BYTES, 384 << 20);
    assert_eq!(OPEN_PACKAGE_BYTES, 512 << 20);
    assert_eq!(OPEN_FILE_BYTES, (512 + 64) << 20, "the file, as the caller holds it, is the package's cap and 64 MiB");
    // A save holds no file, and its worst is the model of the drawing, the tables of its check, the tree of one child, its strings, the blobs and a process's own.
    assert_eq!(
        SAVE_WORST_BYTES,
        OPEN_MODEL_BYTES + SECTION_STREAM_BYTES + OPEN_ELEMENT_ITEMS * TREE_ITEM_BYTES + OPEN_PART_BYTES + OPEN_BLOB_BYTES + BASE_BYTES
    );
    const { assert!(SAVE_WORST_BYTES <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20) };
    assert_eq!(TREE_ITEM_BYTES, 160);
    assert_eq!(ADDRESS_SPACE_BYTES, 3_000_000 * 1024, "what `ulimit -v 3000000` is");
}

#[test]
fn the_open_budget_is_the_pools_for_a_package_of_the_size_the_limits_say() {
    let at = |package: u64| Budget::for_open(&Limits { max_package_bytes: package, ..Limits::default() });
    let open = at(OPEN_PACKAGE_BYTES);
    assert_eq!((open.items, open.bytes, open.blob_bytes), (OPEN_ITEMS, OPEN_MODEL_BYTES, OPEN_BLOB_BYTES));
    // A larger package asks for no more than the pools: they are what fits the process, and not what the package is.
    for package in [4u64 << 30, u64::MAX] {
        let b = at(package);
        assert_eq!((b.items, b.bytes, b.blob_bytes), (OPEN_ITEMS, OPEN_MODEL_BYTES, OPEN_BLOB_BYTES), "a package of {package} bytes");
    }
    // A smaller one gets pools in proportion: an item takes at least a few bytes of the package, a blob its bytes, and the model at most 128 bytes a byte.
    let small = at(1 << 20);
    assert_eq!((small.items, small.bytes, small.blob_bytes), ((1 << 20) / 8, 128 << 20, 1 << 20));
    let tiny = at(1000);
    assert_eq!((tiny.items, tiny.bytes, tiny.blob_bytes), (125, 128_000, 1000));
    assert_eq!((at(0).items, at(0).bytes, at(0).blob_bytes), (0, 0, 0));
    // The pools are the least of the proportion and the figure, each by itself.
    let mid = at(300 << 20);
    assert_eq!((mid.items, mid.bytes, mid.blob_bytes), ((300 << 20) / 8, OPEN_MODEL_BYTES, 300 << 20));
    let below = at(OPEN_BLOB_BYTES - 1);
    assert_eq!(below.blob_bytes, OPEN_BLOB_BYTES - 1);
}

#[test]
fn the_limits_an_open_reads_at_are_the_package_a_part_and_the_one_tree_the_pools_are_sized_for() {
    let limits = open_limits();
    assert_eq!(limits.max_package_bytes, OPEN_PACKAGE_BYTES);
    let defaults = Limits::default();
    // An entry is the container's 256 MiB (a picture is an entry, and the blobs have their own pool); an XML part is held to the codec's own 128 MiB besides.
    assert_eq!(limits.max_part_bytes, defaults.max_part_bytes);
    assert_eq!(limits.max_part_bytes, 256 << 20);
    assert_eq!(limits.xml.max_nodes as u64, OPEN_TREE_ITEMS);
    assert_eq!((limits.max_parts, limits.max_ratio), (defaults.max_parts, defaults.max_ratio));
    assert_eq!(limits.xml.max_attr_bytes, defaults.xml.max_attr_bytes);
    // The open reads as deep as the container ever reads (a part of 200 nested shapes is 205 levels), and no deeper.
    assert_eq!(limits.xml.max_depth, diacraft_container::limits::XML_DEPTH_CEILING);
    assert!(limits.xml.max_depth > defaults.xml.max_depth);
    // The budget of those limits is the budget of the pools: the items of a decode are not the items of a tree.
    let b = Budget::for_open(&limits);
    assert_eq!((b.items, b.bytes, b.blob_bytes), (OPEN_ITEMS, OPEN_MODEL_BYTES, OPEN_BLOB_BYTES));
    const { assert!(OPEN_ITEMS > OPEN_TREE_ITEMS, "a drawing is more than one tree") };
}

// ------------------------------------------------------------------------------------------------------------------------------------- the trees

const TREE: &str = "SHEETXML_POOLS_TREE";

/// The items of a tree, as the XML reader counts them.
fn items_of(e: &El) -> usize {
    let mut n = 0usize;
    let mut stack = vec![e];
    while let Some(e) = stack.pop() {
        n += 1 + e.attrs.len();
        for c in &e.children {
            match c {
                Node::El(c) => stack.push(c),
                _ => n += 1,
            }
        }
    }
    n
}

/// `n` items of the kind `kind`, as the text of a tree, and the bytes of its strings beyond what a small allocation holds (24 bytes), which a tree costs besides its
/// items and the part's bytes are counted for.
fn tree_text(kind: &str, n: usize) -> (String, usize, usize) {
    let long = "v".repeat(60);
    let (unit, per, beyond): (String, usize, usize) = match kind {
        "elements" => ("<a/>".into(), 1, 0),
        "long-names" => ("<shapesheetname/>".into(), 1, 0),
        "attribute" => ("<a b=\"1\"/>".into(), 2, 0),
        "attributes3" => ("<a b=\"1\" c=\"2\" d=\"3\"/>".into(), 4, 0),
        "attributes5" => ("<a b=\"1\" c=\"2\" d=\"3\" e=\"4\" f=\"5\"/>".into(), 6, 0),
        "attributes8" => ("<a b=\"1\" c=\"2\" d=\"3\" e=\"4\" f=\"5\" g=\"6\" h=\"7\" i=\"8\"/>".into(), 9, 0),
        "text" => ("<a>x</a>".into(), 2, 0),
        "comment" => ("<a><!--x--></a>".into(), 2, 0),
        "instruction" => ("<a><?t d?></a>".into(), 2, 0),
        "attribute-and-text" => ("<a b=\"1\">x</a>".into(), 3, 0),
        "pairs" => ("<a><b/></a>".into(), 2, 0),
        "triples" => ("<a><b><c/></b></a>".into(), 3, 0),
        "chains-of-8" => (format!("{}{}", "<a>".repeat(8), "</a>".repeat(8)), 8, 0),
        "chains-of-200" => (format!("{}{}", "<a>".repeat(200), "</a>".repeat(200)), 200, 0),
        // An element with three children, which has room for four, and one with three attributes.
        "children3" => ("<a><b/><b/><b/></a>".into(), 4, 0),
        // The flat lists that a parse grows: four million siblings and a little over (a list that doubles makes room for eight million, twice what it holds), and an
        // element with a million attributes of names of its own.
        "siblings-over-a-power-of-two" => ("<a/>".into(), 1, 0),
        "attributes-of-one-element" => (String::new(), 1, 0),
        // Values of 60 bytes: each is an allocation of 80 bytes, 48 more than a small one, and those are the bytes a part's own are counted for.
        "long-values" => (format!("<a b=\"{long}\" c=\"{long}\" d=\"{long}\"/>"), 4, 3 * 48),
        other => panic!("no tree {other}"),
    };
    let units = n / per;
    if kind == "attributes-of-one-element" {
        let attrs: String = (0..n).map(|i| format!(" a{i}=\"1\"")).collect();
        return (format!("<r{attrs}/>"), n + 1, 0);
    }
    (format!("<r>{}</r>", unit.repeat(units)), units * per + 1, units * beyond)
}

/// What the address space may grow by beyond what a tree's items cost, while a part is parsed: the allocator reserves its arenas in steps of 64 MiB for the thread
/// that parses, and keeps a margin at the top of its heap. The figure the budget counts with for a process before it opens anything ([`BASE_BYTES`]) is for what a
/// process has before; this is the step an arena makes while the work is done.
#[cfg(target_os = "linux")]
const ARENA_STEP_BYTES: u64 = 96 << 20;

#[test]
#[ignore = "run by the test below in a process of its own"]
fn tree_child() {
    let spec = std::env::var(TREE).unwrap_or_default();
    let (kind, n) = spec.split_once(':').expect("kind:items");
    let (text, _, beyond) = tree_text(kind, n.parse().expect("a count"));
    let limits = XmlLimits { max_nodes: usize::MAX, ..native_limits() };
    // **The peak while the tree is made**, not what it is once it is made: a list that grows holds more than it needs until its element is done, and one that
    // moves holds the old and the new. The peak is the highest the resident size and the address space (what `ulimit -v` counts) were, over what the process held
    // with the text of the part.
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak resident size");
    let (vm_before, rss_before) = (status_kib("VmSize"), status_kib("VmRSS"));
    let tree = parse_with(text.as_bytes(), &limits).unwrap();
    let (vm_peak, rss_peak) = (status_kib("VmPeak"), status_kib("VmHWM"));
    drop(text);
    let items = items_of(&tree) as u64;
    // What the tree holds beside what its items cost: the strings beyond a small allocation, which the part's bytes are counted for.
    let resident = (rss_peak.saturating_sub(rss_before) * 1024).saturating_sub(beyond as u64);
    let virtual_peak = (vm_peak.saturating_sub(vm_before) * 1024).saturating_sub(beyond as u64);
    println!("TREE kind={kind} items={items} per_item={} virtual_peak={virtual_peak}", resident / items);
    std::hint::black_box(&tree);
}

#[test]
#[cfg(target_os = "linux")]
fn a_tree_costs_no_more_an_item_than_the_figure_the_open_budget_counts_with_and_the_worst_is_near_it() {
    let exe = std::env::current_exe().unwrap();
    let mut worst = (0u64, "");
    let mut over: Vec<String> = Vec::new();
    for kind in [
        "elements",
        "long-names",
        "attribute",
        "attributes3",
        "attributes5",
        "attributes8",
        "text",
        "comment",
        "instruction",
        "attribute-and-text",
        "pairs",
        "triples",
        "chains-of-8",
        "chains-of-200",
        "long-values",
        "children3",
        "siblings-over-a-power-of-two",
        "attributes-of-one-element",
    ] {
        let items = match kind {
            "siblings-over-a-power-of-two" => 4_194_305,
            "attributes-of-one-element" => 1_000_001,
            _ => 1_500_000,
        };
        let out = Command::new(&exe)
            .args(["--exact", "model_pools::tree_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env(TREE, format!("{kind}:{items}"))
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let line =
            text.lines().find_map(|l| l.find("TREE kind=").map(|at| l[at..].to_owned())).unwrap_or_else(|| panic!("no measure for {kind}: {text}"));
        let field = |key: &str| -> u64 { line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number") };
        let (per, vm, counted) = (field("per_item="), field("virtual_peak="), field("items="));
        eprintln!("{kind:<30} {per} bytes an item resident at the peak, the address space grew {} bytes an item", vm / counted);
        if per > TREE_ITEM_BYTES {
            over.push(format!("{kind}: a tree costs {per} bytes an item at its peak and the budget counts with {TREE_ITEM_BYTES}"));
        }
        if vm > counted * TREE_ITEM_BYTES + ARENA_STEP_BYTES {
            over.push(format!("{kind}: the address space grew {vm} bytes for {counted} items while the tree was made, and the budget counts with {TREE_ITEM_BYTES} an item"));
        }
        if per > worst.0 {
            worst = (per, Box::leak(kind.to_owned().into_boxed_str()));
        }
    }
    assert!(over.is_empty(), "{over:#?}");
    // The figure is the worst case and not a round number far above it: the worst kind is within a tenth of it.
    assert!(
        worst.0 * 10 >= TREE_ITEM_BYTES * 9,
        "the worst tree measured is {} bytes an item ({}), the budget counts with {TREE_ITEM_BYTES}",
        worst.0,
        worst.1
    );
}

// ------------------------------------------------------------------------------------------------------------------------------------- the process

#[test]
#[ignore = "run by the test below in a process of its own"]
fn base_child() {
    println!("BASE vm_peak={}", status_kib("VmPeak") * 1024);
}

#[test]
#[cfg(target_os = "linux")]
fn a_process_holds_no_more_before_it_opens_a_drawing_than_the_figure_the_open_budget_counts_with() {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe).args(["--exact", "model_pools::base_child", "--ignored", "--nocapture", "--test-threads=1"]).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let peak: u64 =
        text.lines().find_map(|l| l.find("BASE vm_peak=").map(|at| l[at + 13..].trim().to_owned())).and_then(|v| v.parse().ok()).expect("a peak");
    eprintln!("a process before it opens anything: {} MiB of address space", peak >> 20);
    assert!(peak <= BASE_BYTES, "{peak} bytes of address space before any work; the budget counts with {BASE_BYTES}");
    assert!(peak * 4 >= BASE_BYTES * 3 / 2 || peak > 0, "the figure is of the right order");
}

// ------------------------------------------------------------------------------------------------------------------------------------- everything at once

const SCENARIO: &str = "SHEETXML_POOLS_SCENARIO";

/// The blobs of the scenario: byte strings of a size each, all of them held by the source, so that the process has them as an open does.
struct Blobs(Vec<Arc<[u8]>>);

impl BlobSource for Blobs {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.0.get(id.0 as usize).cloned()
    }
}

/// A source that makes each part's text when it is asked for it, parses it at the limits of an open, and lets the text go: a part's bytes are held while its tree
/// is made, as an open holds them, and nothing of a part is held once its tree is let go.
struct Made<'a> {
    pages: &'a dyn Fn(usize) -> String,
    count: usize,
    /// Whether the part is given as bytes, to be read as a stream of its children (a page of more items than a tree may have), and not as a tree.
    stream: bool,
}

impl PartSource for Made<'_> {
    type Tree = El;

    fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        if index >= self.count {
            return Err(CodecError::Malformed { part: "page".to_owned(), why: "no such part" });
        }
        let text = (self.pages)(index);
        parse_with(text.as_bytes(), &open_limits().xml).map_err(|e| {
            eprintln!("the part {index} was refused by the reader: {e}");
            CodecError::Malformed { part: "page".to_owned(), why: "a part the reader refuses" }
        })
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "master".to_owned(), why: "no such part" })
    }

    fn page_bytes(&mut self, index: usize, _name: &str) -> Result<Option<std::borrow::Cow<'_, [u8]>>, CodecError> {
        if !self.stream || index >= self.count {
            return Ok(None);
        }
        Ok(Some(std::borrow::Cow::Owned((self.pages)(index).into_bytes())))
    }
}

fn parse(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap_or_else(|e| panic!("parse: {e}"))
}

/// A decode with every pool at its limit: the blobs fill the blob pool; the shapes of the first parts build the model until it is nearly the pool; the last part is
/// the worst tree the items allow (elements nested to 200 levels, and long strings that fill the part's bytes), **made while the model and the blobs are held**,
/// which the decode passes over. The process is held to 3,000,000 KiB: a refusal in words is what it ends in, and an abort is the failure.
#[test]
#[ignore = "run by the test below under `ulimit -v`; alone it needs the scenario in the environment"]
fn pools_child() {
    // The file the package was read from is held as a slice while the container reads it (`OPEN_FILE_BYTES`: the package's cap and 64 MiB), beside everything else.
    let _file: Option<Vec<u8>> = (std::env::var(SCENARIO).as_deref() == Ok("pools_with_file")).then(|| vec![0xA5u8; OPEN_FILE_BYTES as usize]);
    let blob_mib = 8usize;
    let nblobs = (OPEN_BLOB_BYTES >> 20) as usize / blob_mib;
    let mut blobs: Vec<Arc<[u8]>> = Vec::new();
    let mut records = String::new();
    for i in 0..nblobs {
        let mut bytes = vec![0u8; blob_mib << 20];
        for (j, b) in bytes.iter_mut().enumerate() {
            *b = (j.wrapping_mul(31) ^ i) as u8;
        }
        let stamp = stamp_of_bytes(&bytes).text();
        records.push_str(&format!("<blob id=\"{i}\" path=\"blobs/{i}.bin\" media-type=\"a/b\" size=\"{}\" stamp=\"{stamp}\"/>", bytes.len()));
        blobs.push(Arc::from(bytes));
    }
    // The model: the first part shows the pictures, which is what asks for the blobs, and then plain shapes at their price each; the second part is plain shapes,
    // and between them they are about nine tenths of the pool.
    let pictures: String = (0..nblobs)
        .map(|i| {
            format!("<shape id=\"{i}\"><foreign rel-id=\"r\" text=\"t\"><picture part=\"/media/p{i}.png\" type=\"image/png\" blob=\"{i}\"/></foreign></shape>")
        })
        .collect();
    let plain = |n: usize| "<shape/>".repeat(n);
    // The filler: chains of 200 nested elements in a place the decoder counts and does not keep, with strings that fill the part's bytes.
    let chains = 9_500usize;
    let value = "s".repeat((OPEN_PART_BYTES as usize - 60_000_000) / chains);
    let filler = move || {
        let chain = format!("<zz v=\"{value}\">{}{}</zz>", "<a>".repeat(199), "</a>".repeat(199));
        format!("<page xmlns=\"{URN}\"><shape id=\"1\"><sheet>{}</sheet></shape></page>", chain.repeat(chains))
    };
    let first = format!("<page xmlns=\"{URN}\">{pictures}{}</page>", plain(950_000));
    let second = format!("<page xmlns=\"{URN}\">{}</page>", plain(950_000));
    let make = move |i: usize| match i {
        0 => first.clone(),
        1 => second.clone(),
        _ => filler(),
    };
    let document = parse(&format!("<document xmlns=\"{URN}\">{records}<page-ref/><page-ref/><page-ref/></document>"));
    let source = Blobs(blobs);
    eprintln!("before the decode: VmRSS {} MiB, VmPeak {} MiB", status_kib("VmRSS") >> 10, status_kib("VmPeak") >> 10);
    let mut budget = Budget::for_open(&open_limits());
    let mut notes: Vec<DecodeNote> = Vec::new();
    let out = decode_from(&document, &mut Made { pages: &make, count: 3, stream: false }, &source, &mut budget, &mut notes);
    let spent = (OPEN_ITEMS - budget.items, OPEN_MODEL_BYTES - budget.bytes, OPEN_BLOB_BYTES - budget.blob_bytes);
    let refusal = out.as_ref().err().map(|e| e.to_string());
    println!(
        "POOLS charged items {} model {} MiB blobs {} MiB, VmPeak {} MiB, VmHWM {} MiB, refused: {}",
        spent.0,
        spent.1 >> 20,
        spent.2 >> 20,
        status_kib("VmPeak") >> 10,
        status_kib("VmHWM") >> 10,
        refusal.as_deref().unwrap_or("no")
    );
    assert!(spent.2 >= OPEN_BLOB_BYTES * 9 / 10, "the blobs are charged at their pool: {} MiB", spent.2 >> 20);
    assert!(spent.1 >= OPEN_MODEL_BYTES * 8 / 10, "the model is charged to near its pool before the last part: {} MiB", spent.1 >> 20);
    match out {
        Ok(_) => {}
        Err(CodecError::Limit { .. }) => {}
        Err(other) => panic!("a refusal that is not the budget's: {other}"),
    }
    println!("pools at their limits: no abort");
}

#[test]
#[cfg(target_os = "linux")]
fn every_pool_at_its_limit_at_once_beside_a_file_of_the_most_an_open_reads_ends_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_pools::pools_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, "pools_with_file").output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("pools at their limits: no abort"), "{text}");
    let peak = text
        .lines()
        .find(|l| l.contains("POOLS"))
        .and_then(|l| l.split("VmPeak ").nth(1))
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok());
    eprintln!("{}", text.lines().filter(|l| l.contains("POOLS")).collect::<Vec<_>>().join("\n"));
    assert!(peak.is_some_and(|p| p < (ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20) >> 20), "{peak:?}");
}

#[test]
#[cfg(target_os = "linux")]
fn every_pool_at_its_limit_at_once_with_the_worst_trees_ends_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_pools::pools_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, "pools").output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("pools at their limits: no abort"), "{text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("POOLS") || l.contains("before the decode")).collect::<Vec<_>>().join("\n"));
}

// ------------------------------------------------------------------------------------------------------------------------------------- sparse places, at the pool

/// The part of a drawing of places that hold one thing each: a page, and how many of its items one part of this kind holds at the most
/// within the caps of the model (a million shapes to a part, 320,000 rows to a section, 8,192 sections to a sheet). **Every unit is its own**: a value that nothing
/// else has (`part` and the unit's number), so that nothing of it is shared and the decode builds and charges all of it, which is the worst a file can make.
fn sparse_part(kind: &str, part: usize) -> (String, usize) {
    let cells = |k: usize, tag: &str| (0..k).map(|c| format!("<Cell N=\"c{c}\" V=\"{tag}\"/>")).collect::<String>();
    match kind {
        // Rows of one cell each in one section: the rows are dense, and the map of each row's cells holds one.
        "rows_of_one_cell" => {
            let rows: String = (0..320_000).map(|i| format!("<Row N=\"r{i}\">{}</Row>", cells(1, &format!("{part}-{i}")))).collect();
            (format!("<shape id=\"1\"><sheet><Section N=\"User\">{rows}</Section></sheet></shape>"), 5 * 320_000 + 6)
        }
        // Sections of one cell each, eight to a sheet.
        "sections_of_one_cell" => {
            let shapes: String = (0..80_000)
                .map(|i| {
                    let sections: String = (0..8).map(|s| format!("<Section N=\"s{s}\">{}</Section>", cells(1, &format!("{part}-{i}-{s}")))).collect();
                    format!("<shape id=\"{i}\"><sheet>{sections}</sheet></shape>")
                })
                .collect();
            (shapes, 80_000 * 44)
        }
        // A sheet that holds one cell, in a shape of its own.
        "sheets_of_one_cell" => {
            ((0..600_000).map(|i| format!("<shape id=\"{i}\"><sheet>{}</sheet></shape>", cells(1, &format!("{part}-{i}")))).collect(), 600_000 * 6)
        }
        // A section that holds one cell directly, in a shape of its own.
        "sections_with_one_direct_cell" => (
            (0..400_000).map(|i| format!("<shape id=\"{i}\"><sheet><Section N=\"User\">{}</Section></sheet></shape>", cells(1, &format!("{part}-{i}")))).collect(),
            400_000 * 8,
        ),
        // A shape of a row of one cell, one of text and one of held values: the places that make a map and a vector of room for four, together.
        "mix" => (
            (0..200_000)
                .map(|i| {
                    format!(
                        "<shape id=\"{i}\"><sheet><Section N=\"User\"><Row N=\"r\">{}</Row></Section></sheet><text><ext><odf><paragraph><run/></paragraph><held group=\"fill\"/></odf></ext></text></shape>",
                        cells(1, &format!("{part}-{i}"))
                    )
                })
                .collect(),
            200_000 * 17,
        ),
        other => panic!("no sparse kind {other}"),
    }
}

/// The items a sparse scenario feeds a decode: more than fill the model's pool with the least a unit costs (the pool is 960 MiB; a unit of this kind is charged at
/// least 130 bytes an item), so that every scenario ends refused in words by the pool.
const SPARSE_ITEMS: usize = 9_500_000;

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn sparse_child() {
    let kind = std::env::var(SCENARIO).unwrap_or_default();
    let items = sparse_part(&kind, 0).1;
    // As many parts as the items need, each made when the decode asks for it: one tree is held at a time.
    let parts = (SPARSE_ITEMS / items.max(1)).max(1);
    let make = |i: usize| format!("<page xmlns=\"{URN}\">{}</page>", sparse_part(&kind, i).0);
    let document = parse(&format!("<document xmlns=\"{URN}\">{}</document>", "<page-ref/>".repeat(parts)));
    let mut budget = Budget::for_open(&open_limits());
    let out = decode_from(&document, &mut Made { pages: &make, count: parts, stream: true }, &Blobs(Vec::new()), &mut budget, &mut Vec::new());
    println!(
        "SPARSE kind={kind} parts={parts} items_charged={} model_charged={} MiB, VmPeak {} MiB, {}",
        OPEN_ITEMS - budget.items,
        (OPEN_MODEL_BYTES - budget.bytes) >> 20,
        status_kib("VmPeak") >> 10,
        out.as_ref().err().map_or_else(|| "decoded".to_owned(), |e| format!("refused: {e}"))
    );
    match out {
        Ok(_) | Err(CodecError::Limit { .. }) => {}
        Err(other) => panic!("a refusal that is not the budget's: {other}"),
    }
    println!("sparse places at the pool: no abort");
}

#[cfg(target_os = "linux")]
fn run_sparse(kind: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_pools::sparse_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, kind).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("no abort"), "{kind}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("SPARSE")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn rows_of_one_cell_at_the_item_limit_end_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    run_sparse("rows_of_one_cell");
}

#[test]
#[cfg(target_os = "linux")]
fn sections_of_one_cell_at_the_item_limit_end_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    run_sparse("sections_of_one_cell");
}

#[test]
#[cfg(target_os = "linux")]
fn shapes_whose_sheet_holds_one_cell_at_the_item_limit_end_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    run_sparse("sheets_of_one_cell");
}

#[test]
#[cfg(target_os = "linux")]
fn sections_with_one_direct_cell_at_the_item_limit_end_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    run_sparse("sections_with_one_direct_cell");
}

#[test]
#[cfg(target_os = "linux")]
fn a_mix_of_sparse_places_at_the_item_limit_ends_in_a_decode_or_a_refusal_and_not_an_abort_under_an_address_limit() {
    run_sparse("mix");
}

// ------------------------------------------------------------------------------------------------------------------------------------- the pool is lent, and a tree is held back

#[test]
fn a_model_with_no_pictures_has_the_blobs_pool_too_and_the_two_never_pass_what_the_pools_are_together() {
    let words = |e: CodecError| match e {
        CodecError::Limit { what, max } => (what, max),
        other => panic!("not a limit: {other:?}"),
    };
    // No pictures: the model may take the whole of what the two pools are, and no more; the refusal says that figure.
    let mut b = Budget::for_open(&open_limits());
    b.charge_bytes(OPEN_POOL_BYTES).unwrap();
    assert_eq!((b.bytes, b.blob_bytes), (0, 0), "the model's own pool is spent first, and then the room the blobs had");
    assert_eq!(words(b.charge_bytes(1).unwrap_err()), ("bytes of the model in memory", OPEN_POOL_BYTES));
    assert_eq!(words(b.charge_blob(1).unwrap_err()), ("bytes of pictures and parts in the drawing", OPEN_BLOB_BYTES));
    // Pictures first: they keep their room, and the model has what is left.
    let mut b = Budget::for_open(&open_limits());
    b.charge_blob(OPEN_BLOB_BYTES).unwrap();
    b.charge_bytes(OPEN_MODEL_BYTES).unwrap();
    assert_eq!(words(b.charge_bytes(1).unwrap_err()).0, "bytes of the model in memory");
    // Half the pictures: the model has the model's pool and the other half.
    let mut b = Budget::for_open(&open_limits());
    b.charge_blob(OPEN_BLOB_BYTES / 2).unwrap();
    b.charge_bytes(OPEN_MODEL_BYTES + OPEN_BLOB_BYTES / 2).unwrap();
    assert!(b.charge_bytes(1).is_err() && b.charge_blob(1).is_err(), "the pools together are spent");
    // A budget that is not lent (a test's, or a caller's own) keeps its pools apart.
    let mut own = Budget::new(10, 100, 100);
    assert!(own.charge_bytes(101).is_err());
    assert_eq!(words(own.charge_bytes(101).unwrap_err()), ("bytes of the model in memory", 100));
}

#[test]
fn the_tree_of_a_document_part_is_held_back_from_the_pool_beyond_what_is_free_and_given_back_when_the_decode_is_done() {
    let mut b = Budget::for_open(&open_limits());
    // Within what is free nothing is held back.
    assert_eq!(b.reserve_tree(OPEN_DOCUMENT_FREE_ITEMS).unwrap(), 0);
    // Beyond it, the tree is taken from the room, at an item's bytes each, and no charge leaves the model and the tree past the pool.
    let held = b.reserve_tree(OPEN_DOCUMENT_FREE_ITEMS + 1_000_000).unwrap();
    assert_eq!(held, 1_000_000 * TREE_ITEM_BYTES);
    assert!(b.charge_bytes(OPEN_POOL_BYTES - held + 1).is_err(), "one byte past what the tree leaves");
    b.charge_bytes(OPEN_POOL_BYTES - held).unwrap();
    b.release_tree(held);
    // Given back, the room is the pool's again.
    b.charge_bytes(held).unwrap();
    // A tree that is more than the pool is refused where it is held back, in the words of the pool.
    let mut tight = Budget::for_open(&open_limits());
    assert!(tight.reserve_tree(OPEN_POOL_BYTES / TREE_ITEM_BYTES + OPEN_DOCUMENT_FREE_ITEMS + 1).is_err());
}

#[test]
fn a_child_of_more_than_the_tree_pool_is_held_back_beyond_it_while_it_is_read_and_what_the_rooms_cannot_hold_is_not_a_child_it_takes() {
    // B1: a child of a part read as a stream may have the items of the tree pool and what the model's room holds beside it, up to the child limit.
    let mut b = Budget::for_open(&open_limits());
    assert_eq!(b.child_items(), OPEN_ELEMENT_ITEMS, "an empty model has room for the most a child may have");
    const { assert!(OPEN_ELEMENT_ITEMS > OPEN_TREE_ITEMS) };
    // Within the tree pool nothing is held back; beyond it, the tree is taken from the room at an item's bytes each, and no charge leaves the model and the tree past the pools.
    assert_eq!(b.reserve_child(OPEN_TREE_ITEMS).unwrap(), 0);
    let held = b.reserve_child(OPEN_TREE_ITEMS + 500_000).unwrap();
    assert_eq!(held, 500_000 * TREE_ITEM_BYTES);
    assert!(b.charge_bytes(OPEN_POOL_BYTES - held + 1).is_err(), "one byte past what the tree leaves");
    b.charge_bytes(OPEN_POOL_BYTES - held).unwrap();
    b.release_tree(held);
    b.charge_bytes(held).unwrap();
    // A model with little room left: a child may have the tree pool's items and what the room holds, and that is told before the child is read.
    let mut small = Budget::new(10_000_000, 16_000_000, 0);
    assert_eq!(small.child_items(), OPEN_TREE_ITEMS + 100_000);
    small.charge_bytes(8_000_000).unwrap();
    assert_eq!(small.child_items(), OPEN_TREE_ITEMS + 50_000, "the less room, the fewer the items");
    assert!(small.reserve_child(OPEN_TREE_ITEMS + 50_001).is_err(), "a child that the room cannot hold is refused, in the words of the pool");
    assert!(small.reserve_child(OPEN_TREE_ITEMS + 50_000).is_ok());
    assert_eq!(small.child_items(), OPEN_TREE_ITEMS, "what is held back is no room: the next child has the tree pool's items");
    // A caller whose limits hold a tree to less than an open's has children of no more.
    let mut limits = open_limits();
    limits.xml.max_nodes = 1_000;
    let tiny = Budget::for_open(&limits);
    assert_eq!(tiny.child_items(), 1_000);
    assert_eq!(
        Budget::for_open(&Limits::default()).child_items(),
        OPEN_ELEMENT_ITEMS,
        "a container's own six million is more than a tree: the child limit"
    );
}

#[test]
fn the_bytes_of_blobs_a_budget_was_charged_are_counted_apart_from_what_the_model_took_of_their_room() {
    // What the model takes of the blobs' room lowers `blob_bytes` and is no blob; the decode asks `blobs_charged` whether a unit cost one.
    let mut b = Budget::for_open(&open_limits());
    assert_eq!(b.blobs_charged(), 0);
    let lent = 100 << 20;
    b.charge_bytes(OPEN_MODEL_BYTES + lent).unwrap();
    assert_eq!((b.blob_bytes, b.blobs_charged()), (OPEN_BLOB_BYTES - lent, 0), "the room the model took is no blob");
    b.charge_blob(10).unwrap();
    assert_eq!(b.blobs_charged(), 10);
    b.charge_blob(5).unwrap();
    assert_eq!(b.blobs_charged(), 15);
    assert!(b.charge_blob(OPEN_BLOB_BYTES).is_err());
    assert_eq!(b.blobs_charged(), 15, "a refusal charges nothing");
    // A budget whose pools are apart counts the same.
    let mut own = Budget::new(10, 100, 100);
    own.charge_bytes(100).unwrap();
    own.charge_blob(30).unwrap();
    assert_eq!((own.blobs_charged(), own.blob_bytes), (30, 70));
}

#[test]
fn a_blob_is_not_charged_the_room_that_a_tree_held_back_and_the_model_have_counted_on() {
    let words = |e: CodecError| match e {
        CodecError::Limit { what, max } => (what, max),
        other => panic!("not a limit: {other:?}"),
    };
    // A tree is held back, and the model takes all the room that is left: its own pool and then the blobs' unspent room.
    let mut b = Budget::for_open(&open_limits());
    let held = b.reserve_tree(OPEN_DOCUMENT_FREE_ITEMS + 1_000_000).unwrap();
    b.charge_bytes(OPEN_POOL_BYTES - held).unwrap();
    assert_eq!((b.bytes, b.blob_bytes), (0, held), "what is left of the blobs' room is what the tree holds back");
    // A blob takes that room from the tree: refused, in the blobs' words, and nothing is taken.
    let before = b.clone();
    assert_eq!(words(b.charge_blob(1).unwrap_err()), ("bytes of pictures and parts in the drawing", OPEN_BLOB_BYTES));
    assert_eq!(b, before, "a refused charge takes nothing");
    // Once the tree is given back, the room is the blobs' again.
    b.release_tree(held);
    b.charge_blob(held).unwrap();
    assert_eq!(b.blob_bytes, 0);
    // The model took a part of the blobs' room (a hundred bytes under what the pools leave beside the tree): a blob has what is left of it, to the byte, and no more.
    let mut b = Budget::for_open(&open_limits());
    let held = b.reserve_tree(OPEN_DOCUMENT_FREE_ITEMS + 1_000_000).unwrap();
    let lent = 100 << 20;
    b.charge_bytes(OPEN_MODEL_BYTES + lent).unwrap();
    let left = OPEN_BLOB_BYTES - lent - held;
    assert!(b.charge_blob(left + 1).is_err(), "one byte past what the tree leaves");
    b.charge_blob(left).unwrap();
    assert!(b.charge_blob(1).is_err() && b.charge_bytes(1).is_err(), "the pools, with the tree, are spent");
    // A budget whose pools are apart keeps them apart: a tree is held back against the model's pool, and a blob has its own whatever the tree is.
    let mut own = Budget::new(10, 1_000, 100);
    let held = own.reserve_tree(OPEN_DOCUMENT_FREE_ITEMS + 5).unwrap();
    assert_eq!(held, 5 * TREE_ITEM_BYTES);
    own.charge_blob(100).unwrap();
    assert!(own.charge_blob(1).is_err());
    assert!(own.charge_bytes(1_000 - held + 1).is_err());
    own.charge_bytes(1_000 - held).unwrap();
}

#[test]
fn a_formula_is_afforded_by_what_the_model_may_still_be_charged_and_not_by_its_own_pool_alone() {
    // The model's own pool is spent and the room the blobs have not spent is what is left: a formula is read, and refused only when that room cannot hold it.
    let formula = format!("={}", (0..200).map(|i| format!("{i}+")).collect::<String>() + "1");
    let page = |cells: usize| {
        let cells: String = (0..cells).map(|i| format!("<Cell N=\"c{i}\" V=\"1\" F=\"{}\"/>", &formula[1..])).collect();
        parse(&format!("<page xmlns=\"{URN}\"><shape id=\"1\"><sheet>{cells}</sheet></shape></page>"))
    };
    let document = parse(&format!("<document xmlns=\"{URN}\"><page-ref/></document>"));
    let tree = page(40);
    let mut budget = Budget::for_open(&open_limits());
    // Spend the model's own pool, leaving the blobs' room.
    budget.charge_bytes(OPEN_MODEL_BYTES - 1_000_000).unwrap();
    budget.charge_bytes(2_000_000).unwrap();
    assert_eq!(budget.bytes, 0);
    let mut notes = Vec::new();
    let out = diacraft_sheetxml::model::decode(&document, &[&tree], &[], &Blobs(Vec::new()), &mut budget, &mut notes);
    assert!(out.is_ok(), "{:?}", out.err());
}
