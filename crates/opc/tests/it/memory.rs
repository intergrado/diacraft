//! What hostile packages cost, measured under an address-space limit.
//!
//! A limit that is only a number in the code says nothing about memory, and an allocation that fails *aborts* the program (Rust's
//! default). So the claims "a refused package costs nothing" and "memory that runs out is an error and not an abort" are
//! measured: each scenario runs in a child process under `ulimit -v` (a cap on the address space), and the parent checks what the
//! child printed and how it ended. Linux only: `ulimit -v` is the tool, and the other systems have no equivalent that a shell
//! gives, so on them this file is not compiled (the numbers in the crate documentation were measured here). What the limits *do* is
//! tested on every platform, with small limits, in `limits.rs`; what is left to Linux is the real allocation of a gigabyte and the
//! measured bounds.
//!
//! The child is an ignored test (`child`) that the parent runs by name; run alone it fails, and it prints its measurements
//! (`VmPeak`, `VmHWM` from `/proc/self/status`) on a line starting `RESULT`. A machine with no `/proc` fails these tests, loudly:
//! they measure nothing there, and a green run would say they had. A machine that refuses the reset of the peak (see `begin_read`)
//! cannot say what a read added, and says so on the standard error where the bound on it would have been checked.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use diacraft_opc::xml::parse;
use diacraft_opc::{Limits, Package};

use crate::zipbuild::{Item, build, many_empty_entries, many_entries, many_entries_with, many_entries_with_unicode_paths, rows_only};
use crate::{content_types_xml, package_with};

const SCENARIO: &str = "OPC_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes. A missing file or line is a failure and not a zero: a measurement that cannot be taken
/// must not look like one that came out small.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("these tests need /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status:\n{text}"))
}

/// Whether the last `begin_read` could reset the peak.
static PEAK_RESET: AtomicBool = AtomicBool::new(false);

/// Before the read that is being measured: forget the peak so far (which holds the cost of building the input) and note what is
/// resident. Linux resets `VmHWM` when `5` is written to `/proc/self/clear_refs`; a system that refuses it (some containers) leaves the
/// peak holding the cost of building the input, and `report_read` then says it did not measure what the read added.
fn begin_read() -> u64 {
    PEAK_RESET.store(std::fs::write("/proc/self/clear_refs", "5").is_ok(), Ordering::Relaxed);
    status_kib("VmRSS")
}

/// Like `report`, with what the read added to what was resident before it (`base`, from `begin_read`): the cost of the read itself, or
/// `unmeasured` when the peak could not be reset.
fn report_read(what: &str, base: u64) {
    let extra = if PEAK_RESET.load(Ordering::Relaxed) { format!("{}KiB", status_kib("VmHWM").saturating_sub(base)) } else { "unmeasured".to_owned() };
    println!("\nRESULT {what} ReadExtra={extra} VmHWM={}KiB", status_kib("VmHWM"));
}

fn report(what: &str) {
    println!("\nRESULT {what} VmPeak={}KiB VmHWM={}KiB", status_kib("VmPeak"), status_kib("VmHWM"));
}

/// The scenarios the child runs, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        // Headers that claim sizes from 2 GiB to 16 EiB for a few hundred bytes: refused with nothing allocated for them.
        "refuse_big_claims" => {
            let junk = vec![9u8; 4096];
            let mut classes = Vec::new();
            for claim in [2u64 << 30, 1 << 36, 1 << 50, u64::MAX] {
                let bytes = package_with(vec![Item::stored("a.bin", &junk).claiming_size(claim)]);
                classes.push(Package::read(&bytes, &Limits::default()).map(|_| "read").unwrap_or_else(|e| e.class()));
            }
            report(&classes.join(","));
        }
        // Real data, over what the address space holds: part after part of 32 MiB, ten of them, each honest and within the
        // limits, under a cap that holds six. Memory runs out and the answer is an error.
        "inflate_past_the_address_space" => {
            let parts: Vec<Item> = (0..10).map(|i| Item::zeros(&format!("p{i}.bin"), 32 * 1024 * 1024)).collect();
            let bytes = package_with(parts);
            let limits = Limits { max_ratio: 2000, ..Limits::default() };
            let result = Package::read(&bytes, &limits);
            report(&result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned()));
        }
        // The same, with a header that lies low (it says 10 bytes): the buffer is not reserved from the claim, so it grows as the
        // bytes come, and growth that fails is an error as well. 200 MiB of zeros under an address space that holds about 160 MiB.
        "inflate_past_the_address_space_with_a_lying_header" => {
            let bytes = package_with(vec![Item::zeros("big.bin", 200 * 1024 * 1024).claiming_size(10)]);
            let limits = Limits { max_ratio: 2000, ..Limits::default() };
            let result = Package::read(&bytes, &limits);
            report(&result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned()));
        }
        // One part at the per-part limit's order of size, to see what an honest large part costs. 64 MiB here.
        "one_large_part" => {
            let bytes = package_with(vec![Item::zeros("big.bin", 64 * 1024 * 1024)]);
            let limits = Limits { max_ratio: 2000, ..Limits::default() };
            let p = Package::read(&bytes, &limits);
            report(&p.map(|p| format!("read {} bytes", p.part("big.bin").map(<[u8]>::len).unwrap_or(0))).unwrap_or_else(|e| e.class().to_owned()));
        }
        // A million entries, 90 MB: refused as too many parts. What did the directory cost before that was known?
        "million_entries" => {
            let ct = content_types_xml(&[("xml", "application/xml")], &[]);
            let bytes = many_empty_entries(1_000_000, ct.as_bytes());
            println!("\nINPUT {} bytes", bytes.len());
            let result = Package::read(&bytes, &Limits::default());
            report(&result.map(|_| "read".to_owned()).unwrap_or_else(|e| e.class().to_owned()));
        }
        // Fifty thousand real entries, the part limit exactly: the most the spec lets a directory hold.
        "fifty_thousand_entries" => {
            let ct = content_types_xml(&[("xml", "application/xml")], &[]);
            let bytes = many_empty_entries(50_000, ct.as_bytes());
            println!("\nINPUT {} bytes", bytes.len());
            let result = Package::read(&bytes, &Limits::default());
            report(&result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned()));
        }
        // The largest tree the default limits allow: just under six million items.
        "largest_xml_tree" => {
            let n = 1_950_000; // `<b a="1" c="2"/>`: three items each, 5.85 million
            let doc = format!("<a>{}</a>", "<b a=\"1\" c=\"2\"/>".repeat(n));
            println!("\nINPUT {} bytes", doc.len());
            let result = parse(doc.as_bytes());
            report(&result.map(|el| format!("parsed {} children", el.elements().count())).unwrap_or_else(|e| e.class().to_owned()));
        }
        "tree_over_the_limit" => {
            let doc = format!("<a>{}</a>", "<b a=\"1\" c=\"2\"/>".repeat(2_100_000));
            let result = parse(doc.as_bytes());
            report(&result.map(|_| "parsed".to_owned()).unwrap_or_else(|e| e.class().to_owned()));
        }
        // 1,000 rows, each with 64 KB of empty extra records (16,000 of them): a file of 64 MB (the largest real one) that the reader walks
        // for what its extra fields hold. Nothing of them is kept, so what it costs is the file and the time to look.
        "extra_records_64mb" => {
            let ct = content_types_xml(&[("xml", "application/xml")], &[]);
            let bytes = many_entries((0..1000).map(|i| format!("p{i}").into_bytes()), 65_480, ct.as_bytes());
            println!("\nINPUT {} bytes", bytes.len());
            let base = begin_read();
            let start = std::time::Instant::now();
            let result = Package::read(&bytes, &Limits::default());
            let seconds = start.elapsed().as_secs_f64();
            report_read(
                &format!("{} in {seconds:.2}s", result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned())),
                base,
            );
        }
        // Rows behind a stub, and past the row limit: 1.5 million rows of 50 bytes (75 MB), refused from the count before a row is read.
        "rows_behind_a_stub" => {
            let behind = rows_only(1_500_000, 1);
            println!("\nINPUT {} bytes", behind.len());
            let base = begin_read();
            let start = std::time::Instant::now();
            let first = Package::read(&behind, &Limits::default()).map(|_| "read").unwrap_or_else(|e| e.class());
            drop(behind);
            let past = rows_only(1_500_000, 0);
            let second = Package::read(&past, &Limits::default()).map(|_| "read").unwrap_or_else(|e| e.class());
            report_read(&format!("{first},{second} in {:.2}s", start.elapsed().as_secs_f64()), base);
        }
        // As many rows as OPC_ROWS says (25,000 by default; the row limit is 100,016), each with the longest name a part may have, half of
        // them folders, the rest parts: what the directory read costs at its worst.
        "longest_names" => {
            let rows: usize = std::env::var("OPC_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(25_000);
            let parts = ((rows - 1) / 2).min(50_000);
            let dirs = rows - 1 - parts;
            let names = (0..parts)
                .map(|i| format!("{}{i:08}.bin", "f".repeat(1024 - 12)).into_bytes())
                .chain((0..dirs).map(|i| format!("{}{i:08}/", "d".repeat(1024 - 9)).into_bytes()));
            let ct = content_types_xml(&[("xml", "application/xml")], &[]);
            let bytes = many_entries(names, 0, ct.as_bytes());
            println!("\nINPUT {} bytes, {rows} rows", bytes.len());
            let base = begin_read();
            let start = std::time::Instant::now();
            let result = Package::read(&bytes, &Limits::default());
            let seconds = start.elapsed().as_secs_f64();
            report_read(
                &format!("{} in {seconds:.2}s", result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned())),
                base,
            );
        }
        // The names as written are kept as well when a Unicode path field gives a row another name, so the worst is 50,000 parts
        // that each have both, of 1,016 bytes each (the limit is 1,024), and a field of the same size in each row's extra field. OPC_FIELDS=0
        // makes the same rows with no field (the name is then kept once), for the difference.
        "names_with_fields" => {
            let fields = std::env::var("OPC_FIELDS").map(|v| v != "0").unwrap_or(true);
            let ct = content_types_xml(&[("xml", "application/xml")], &[]);
            let written = |i: u32| format!("{}{i:08}", "w".repeat(1008)).into_bytes();
            let bytes = if fields {
                many_entries_with_unicode_paths((0..50_000).map(|i| (written(i), format!("{}{i:08}", "f".repeat(1008)))), ct.as_bytes(), false)
            } else {
                many_entries_with((0..50_000).map(written), 0, ct.as_bytes(), false)
            };
            println!("\nINPUT {} bytes", bytes.len());
            let base = begin_read();
            let start = std::time::Instant::now();
            let result = Package::read(&bytes, &Limits::default());
            let seconds = start.elapsed().as_secs_f64();
            report_read(
                &format!("{} in {seconds:.2}s", result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned())),
                base,
            );
        }
        // Behaviourally: four parts of 256 MiB (the part limit) are 1 GiB, the package limit, and read; a fifth is refused by
        // default, from the headers, before anything is inflated; and a larger `Limits` opens all five. (The parts are zeros at a ratio of
        // about 518, under the cap of 1,000; the archive of five is 3 MB.)
        "package_cap" => {
            let part = |i: usize| Item::zeros_under_the_ratio_cap(&format!("p{i}.bin"), 256 * 1024 * 1024);
            let four = package_with((0..4).map(part).collect());
            let five = package_with((0..5).map(part).collect());
            println!("\nINPUT {} and {} bytes", four.len(), five.len());
            let base = begin_read();
            let read_four =
                Package::read(&four, &Limits::default()).map(|p| p.parts().filter(|n| p.part(n).map(<[u8]>::len) == Some(256 << 20)).count());
            let refused = Package::read(&five, &Limits::default()).map(|_| "read").unwrap_or_else(|e| e.class());
            let roomy = Limits { max_package_bytes: 2 * 1024 * 1024 * 1024, ..Limits::default() };
            let read_five = Package::read(&five, &roomy).map(|p| p.parts().count());
            let show = |r: Result<usize, diacraft_opc::OpcError>| r.map(|n| n.to_string()).unwrap_or_else(|e| e.class().to_owned());
            report_read(&format!("{},{refused},{}", show(read_four), show(read_five)), base);
        }
        // The most that names can cost the check that no name is the prefix of another. 50,000 parts (the part limit), each
        // with the longest name a part may have (1,024 bytes) made of 508 folders `a/` and eight digits, so that every comparison of two
        // names runs past a kilobyte of common bytes. Read twice in one process: this, and 50,000 names of the same size with no folder at
        // all, so that the cost of the depth is told from the cost of the names by the same machine at the same moment.
        "deepest_names" => {
            let ct = content_types_xml(&[("xml", "application/xml")], &[]);
            let deep = many_entries_with((0..50_000).map(|i| format!("{}{i:08}", "a/".repeat(508)).into_bytes()), 0, ct.as_bytes(), false);
            let flat = many_entries_with((0..50_000).map(|i| format!("{}{i:08}", "f".repeat(1016)).into_bytes()), 0, ct.as_bytes(), false);
            println!("\nINPUT {} bytes", deep.len());
            // Counted, not timed: the comparisons of names the prefix check makes, and the bytes of names it looks at in them.
            let count = |bytes: &[u8]| {
                diacraft_opc::work::reset();
                let result = Package::read(bytes, &Limits::default());
                let read = result.map(|p| format!("read {} parts", p.parts().count())).unwrap_or_else(|e| e.class().to_owned());
                format!("{read} with {} comparisons of {} bytes", diacraft_opc::work::name_comparisons(), diacraft_opc::work::name_bytes())
            };
            let base = begin_read();
            let (deep_result, flat_result) = (count(&deep), count(&flat));
            report_read(&format!("{deep_result}, flat {flat_result}"), base);
        }
        // A content types stream of about the most the byte limit allows (4 MiB): 60,000 rows of 58 bytes, in a package.
        "largest_content_types" => {
            let defaults: Vec<(String, String)> = (0..60_000).map(|i| (format!("e{i}"), "application/x-t".to_owned())).collect();
            let d: Vec<(&str, &str)> = defaults.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let ct = content_types_xml(&d, &[]);
            println!("\nINPUT {} bytes", ct.len());
            let bytes = build(&[Item::deflated("[Content_Types].xml", ct.as_bytes())]);
            let result = Package::read(&bytes, &Limits::default());
            report(&result.map(|p| format!("read {} rows", p.content_types().defaults.len())).unwrap_or_else(|e| e.class().to_owned()));
        }
        other => panic!("no scenario {other:?}"),
    }
}

/// Run `scenario` in a child under an address-space limit of `limit_kib`; give what it printed and whether it ended normally.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// The text after `RESULT ` in the child's output.
fn result_of(stdout: &str) -> &str {
    stdout.lines().find_map(|l| l.strip_prefix("RESULT ")).unwrap_or_else(|| panic!("the child printed no RESULT line:\n{stdout}"))
}

fn print_measure(scenario: &str, limit_kib: u64, stdout: &str) {
    let start = stdout.lines().find(|l| l.starts_with("START")).unwrap_or("");
    let input = stdout.lines().find(|l| l.starts_with("INPUT")).unwrap_or("");
    eprintln!("MEASURED {scenario} under {limit_kib} KiB: {start} {input} {}", result_of(stdout));
}

#[test]
fn a_header_that_claims_a_huge_size_costs_nothing() {
    let (ok, stdout) = run_child("refuse_big_claims", 400_000);
    assert!(ok, "{stdout}");
    print_measure("refuse_big_claims", 400_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("zip_ratio_exceeded,zip_ratio_exceeded,zip_ratio_exceeded,zip_ratio_exceeded"));
}

#[test]
fn memory_that_runs_out_while_a_part_is_read_is_an_error_and_not_an_abort() {
    // Ten honest parts of 32 MiB (320 MiB), every one within the limits, under an address space that holds a part or two. The
    // allocation is refused by the system and the reader says so. If it were an infallible `Vec` growth the child would abort
    // (SIGABRT) and `ok` would be false.
    let (ok, stdout) = run_child("inflate_past_the_address_space", 300_000);
    assert!(ok, "the child must end normally, not abort: {stdout}");
    print_measure("inflate_past_the_address_space", 300_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("out_of_memory"));
}

#[test]
fn growth_that_runs_out_while_a_part_is_read_is_an_error_too() {
    // The header says 10 bytes and the stream holds 200 MiB: the buffer grows with the bytes, and the growth is refused.
    let (ok, stdout) = run_child("inflate_past_the_address_space_with_a_lying_header", 300_000);
    assert!(ok, "the child must end normally, not abort: {stdout}");
    print_measure("inflate_past_the_address_space_with_a_lying_header", 300_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("out_of_memory"));
}

#[test]
fn the_package_cap_is_one_gib_four_parts_of_the_part_limit_read_and_a_fifth_is_refused() {
    // The child reads four parts of 256 MiB (1 GiB) under an address space of 1.6 GB, is refused the fifth by default, and opens all
    // five with a larger `Limits`. One scenario, so that the first 1 GiB is gone before the 1.25 GiB of the last.
    let (ok, stdout) = run_child("package_cap", 1_600_000);
    assert!(ok, "{stdout}");
    print_measure("package_cap", 1_600_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("4,package_too_large,5"));
    // The parts (1 GiB) and the five of the last read, which the reader dropped before it read the next: measured 1.25 GiB.
    assert_read_extra_under("package_cap", &stdout, 1_400_000);
}

#[test]
fn the_same_package_reads_when_the_address_space_holds_it() {
    // The control for the test above: with room, the same ten parts read. So the error above is memory and not the package.
    let (ok, stdout) = run_child("inflate_past_the_address_space", 1_500_000);
    assert!(ok, "{stdout}");
    print_measure("inflate_past_the_address_space", 1_500_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("read"));
}

#[test]
fn an_honest_large_part_costs_about_its_own_size() {
    let (ok, stdout) = run_child("one_large_part", 700_000);
    assert!(ok, "{stdout}");
    print_measure("one_large_part", 700_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').take(2).collect::<Vec<_>>(), ["read", "67108864"]);
}

#[test]
fn a_million_entry_directory_is_refused_within_a_memory_bound_that_follows_the_file() {
    // The file is 90 MB. The directory of a million entries has to be read to be counted, so the cost follows the file size and
    // not the cap on parts: measured here, and bounded by the limit below.
    let (ok, stdout) = run_child("million_entries", 1_000_000);
    assert!(ok, "{stdout}");
    print_measure("million_entries", 1_000_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("too_many_parts"));
}

#[test]
fn a_directory_of_the_most_entries_the_limit_allows_reads() {
    let (ok, stdout) = run_child("fifty_thousand_entries", 400_000);
    assert!(ok, "{stdout}");
    print_measure("fifty_thousand_entries", 400_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').take(2).collect::<Vec<_>>(), ["read", "50000"]);
}

#[test]
fn the_largest_xml_tree_the_limits_allow_is_built_within_a_measured_bound() {
    let (ok, stdout) = run_child("largest_xml_tree", 2_000_000);
    assert!(ok, "{stdout}");
    print_measure("largest_xml_tree", 2_000_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').take(2).collect::<Vec<_>>(), ["parsed", "1950000"]);
}

#[test]
fn a_tree_over_the_item_limit_is_refused_early_and_costs_less_than_one_within_it() {
    let (ok, stdout) = run_child("tree_over_the_limit", 2_000_000);
    assert!(ok, "{stdout}");
    print_measure("tree_over_the_limit", 2_000_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').next(), Some("xml_too_complex"));
}

/// The seconds a child printed, from `... in 1.23s`.
fn seconds_of(result: &str) -> f64 {
    result.split(" in ").nth(1).and_then(|r| r.split('s').next()).and_then(|r| r.parse().ok()).unwrap_or_else(|| panic!("no time in {result}"))
}

/// What the read added, in KiB, from a `RESULT` line (`ReadExtra=123KiB`), or `None` when the child could not reset its peak and so
/// cannot say.
fn read_extra_kib(result: &str) -> Option<u64> {
    let number = result.split("ReadExtra=").nth(1)?.split("KiB").next()?;
    number.parse().ok()
}

/// The read added less than `limit_kib` to what the child held before it. The bound is the thing the claim "this is never stored" is
/// about: a bound a small multiple of what is measured, and far under what storing it would cost. Where the peak could not be reset the
/// number is not the read's: that is said, and not passed over as a pass.
fn assert_read_extra_under(scenario: &str, stdout: &str, limit_kib: u64) {
    let result = result_of(stdout);
    if result.contains("ReadExtra=unmeasured") {
        eprintln!("NOT CHECKED: {scenario}: this system refused the reset of the peak, so what the read added is not known: {result}");
        return;
    }
    let added = read_extra_kib(result).unwrap_or_else(|| panic!("no ReadExtra in {result}"));
    assert!(added < limit_kib, "{scenario}: the read added {added} KiB, and the bound is {limit_kib}: {result}");
}

#[test]
fn a_64_mb_file_of_extra_fields_is_walked_in_time_and_memory_that_follow_the_file() {
    // Review finding 3: 1,000 rows with 64 KB of empty extra records, a file of 64 MB, took 690 MB with the library this crate used, and
    // died under 400 MB. The extra fields are walked and never kept: bounded here under an address space of 300 MB.
    let (ok, stdout) = run_child("extra_records_64mb", 300_000);
    assert!(ok, "{stdout}");
    print_measure("extra_records_64mb", 300_000, &stdout);
    let result = result_of(&stdout);
    assert!(result.starts_with("read 1000 parts in "), "{result}");
    // A hang guard and no measure of cost (the read takes about a second at load 50; what it costs in memory is bounded below).
    assert!(seconds_of(result) < 60.0, "{result}");
    // Never stored: 1,000 rows of 64 KB of extra fields are 65 MB, and the read added 0.2 MB. Keeping them would add all 65.
    assert_read_extra_under("extra_records_64mb", &stdout, 8 * 1024);
}

#[test]
fn rows_behind_a_stub_and_past_the_row_limit_cost_the_file_and_nothing_more() {
    // Finding 3: 4 million rows behind a stub, 202 MB, took 942 MB and died under 500 MB. Here 1.5 million rows (75 MB) twice (with the first reader: an abort for memory under 400 MB), refused
    // before the first row, under an address space of 400 MB.
    let (ok, stdout) = run_child("rows_behind_a_stub", 400_000);
    assert!(ok, "{stdout}");
    print_measure("rows_behind_a_stub", 400_000, &stdout);
    let result = result_of(&stdout);
    assert!(result.starts_with("zip_corrupt,too_many_parts in "), "{result}");
    // A hang guard, as above.
    assert!(seconds_of(result) < 60.0, "{result}");
    // Refused from the count before a row is read: nothing of the 75 MB is walked or kept (measured: nothing).
    assert_read_extra_under("rows_behind_a_stub", &stdout, 4 * 1024);
}

#[test]
fn a_directory_of_the_longest_names_is_read_within_a_measured_bound() {
    // 25,000 rows of 1,024-byte names (about 52 MB): the numbers for the full 100,016 rows are in the crate docs.
    let (ok, stdout) = run_child("longest_names", 600_000);
    assert!(ok, "{stdout}");
    print_measure("longest_names", 600_000, &stdout);
    let result = result_of(&stdout);
    assert!(result.starts_with("read 12499 parts in "), "{result}");
    // A hang guard, as above.
    assert!(seconds_of(result) < 60.0, "{result}");
    // What is kept is the 12,499 part names (2.1 KB each, measured 26.8 MB). The folders' names are dropped as they are read, and the
    // bound is under what keeping them as well would cost (12,500 of 1 KB: 39 MB).
    assert_read_extra_under("longest_names", &stdout, 32 * 1024);
}

#[test]
fn the_largest_content_types_stream_the_limit_allows_reads() {
    let (ok, stdout) = run_child("largest_content_types", 600_000);
    assert!(ok, "{stdout}");
    print_measure("largest_content_types", 600_000, &stdout);
    assert_eq!(result_of(&stdout).split(' ').take(2).collect::<Vec<_>>(), ["read", "60000"]);
}

/// The comparisons and the bytes of names looked at that the `deepest_names` child counted for the deep names, and for the flat control:
/// from `read 50000 parts with 800000 comparisons of 900000000 bytes, flat read 50000 parts with ...`.
fn deep_and_flat_counts(result: &str) -> ((u64, u64), (u64, u64)) {
    let counts = |part: &str| -> Option<(u64, u64)> {
        let (_, tail) = part.split_once(" with ")?;
        let mut numbers = tail.split_whitespace().filter_map(|w| w.parse::<u64>().ok());
        Some((numbers.next()?, numbers.next()?))
    };
    let (deep, flat) = result.split_once(", flat ").unwrap_or((result, ""));
    match (counts(deep), counts(flat)) {
        (Some(deep), Some(flat)) => (deep, flat),
        _ => panic!("no two counts in {result}"),
    }
}

#[test]
fn the_deepest_names_the_limits_allow_cost_what_flat_names_of_the_same_size_cost() {
    // Review round 2, I3: 50,000 names of 508 folders each (1,024 bytes, a file of 55 MB) took 2.06 s in release and 4.68 s here when the
    // check that no name is the prefix of another compared them segment by segment: seven to eleven times what 50,000 names of the same
    // size and no folders cost. A bound in seconds says nothing on a machine that is not this one (and the ratio of two times moves with the
    // load on it, which failed this test at a load of 46), so the work is counted: the comparisons of names the check makes and the bytes
    // of names it looks at, for the deep names and for the flat control read in the same process. The deep names may cost as much as the
    // flat ones and not twice as much (they cost the same now: the names share a kilobyte either way, skipped a block at a time). A check
    // that hashed every ancestor of every name is 13 GB of hashing and is far past it.
    let (ok, stdout) = run_child("deepest_names", 600_000);
    assert!(ok, "{stdout}");
    print_measure("deepest_names", 600_000, &stdout);
    let result = result_of(&stdout);
    assert!(result.starts_with("read 50000 parts with ") && result.contains(", flat read 50000 parts with "), "{result}");
    let ((deep_comparisons, deep_bytes), (flat_comparisons, flat_bytes)) = deep_and_flat_counts(result);
    // A sort of 50,000 names makes about 17 comparisons for each (twenty at the most), and the walk after it looks at each name once.
    assert!((50_000..=20 * 50_000).contains(&deep_comparisons), "{deep_comparisons} comparisons of 50,000 names: {result}");
    assert!(
        deep_comparisons <= 2 * flat_comparisons,
        "the deep names took {deep_comparisons} comparisons and the flat ones {flat_comparisons}: {result}"
    );
    assert!(deep_bytes <= 2 * flat_bytes, "the deep names took {deep_bytes} bytes looked at and the flat ones {flat_bytes}: {result}");
    // About 2.2 KB for each part, as for any other names of this size.
    assert_read_extra_under("deepest_names", &stdout, 130_000);
}

#[test]
fn fifty_thousand_rows_that_each_have_a_name_as_written_and_a_field_are_read_within_a_measured_bound() {
    // A row with a Unicode path field keeps the folded key of its name as written as well as the name the field gives. At the part
    // limit with names of 1,016 bytes in both places the read adds 161 MB (108 MB for the same rows with no field, which keep each name
    // once); the bound is a quarter above that, and under what keeping a third copy of each name would cost (211 MB).
    let (ok, stdout) = run_child("names_with_fields", 900_000);
    assert!(ok, "{stdout}");
    print_measure("names_with_fields", 900_000, &stdout);
    let result = result_of(&stdout);
    assert!(result.starts_with("read 50000 parts in "), "{result}");
    // A hang guard, as above (this one reads in about three seconds at load 50).
    assert!(seconds_of(result) < 60.0, "{result}");
    assert_read_extra_under("names_with_fields", &stdout, BOUND_WITH_FIELDS_KIB);
}

/// The most that the rows of `names_with_fields` may add to what is held, in KiB.
const BOUND_WITH_FIELDS_KIB: u64 = 200_000;
