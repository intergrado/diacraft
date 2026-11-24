//! **The entries of an archive inflated on threads are the entries inflated one after another** (`Archive::inflate_many`): the same bytes, the same counts of work and, for an
//! archive with a fault, the same error, which is the first entry in the order that fails, at any number of threads.

use diacraft_container::zip::Archive;
use diacraft_container::{ContainerError, Limits, Work};

use crate::zipbuild::{Item, build};

/// Bytes that deflate to a part of their size, made from a seed.
fn bytes(len: usize, mut seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        out.extend(seed.to_le_bytes().iter().map(|b| b & 0x0F));
    }
    out.truncate(len);
    out
}

/// Entries of every size the planner tells apart: small ones, ones past the size a thread is worth, stored and deflated, empty ones and a folder.
fn items() -> Vec<Item> {
    let mut items = Vec::new();
    for k in 0..40usize {
        let len = match k % 5 {
            0 => 100,
            1 => 70_000,
            2 => 0,
            3 => 400_000,
            _ => 3_000,
        };
        let content = bytes(len, 0x9E37_79B9 ^ k as u64);
        items.push(if k % 3 == 0 { Item::stored(&format!("p{k}.bin"), &content) } else { Item::deflated(&format!("p{k}.bin"), &content) });
    }
    items.push(Item::stored("folder/", b""));
    items
}

/// What reading each entry one at a time gives: the bytes, or the error, with the work the entries before it cost.
fn one_at_a_time(file: &[u8], limits: &Limits) -> (Result<Vec<Vec<u8>>, ContainerError>, Work) {
    let mut archive = Archive::read(file, limits).unwrap();
    let mut work = Work::default();
    let mut out = Vec::new();
    for index in 0..archive.entries().len() {
        match archive.inflate(index, &mut work) {
            Ok(data) => out.push(data),
            Err(e) => return (Err(e), work),
        }
    }
    (Ok(out), work)
}

fn on_threads(file: &[u8], limits: &Limits, threads: usize) -> (Result<Vec<Vec<u8>>, ContainerError>, Work) {
    let mut archive = Archive::read(file, limits).unwrap();
    let mut work = Work::default();
    let indices: Vec<usize> = (0..archive.entries().len()).collect();
    let result = archive.inflate_many(&indices, threads, &mut work);
    (result, work)
}

#[test]
fn the_entries_are_the_same_bytes_and_cost_the_same_work_at_any_number_of_threads() {
    let file = build(&items());
    let limits = Limits::default();
    let (expected, expected_work) = one_at_a_time(&file, &limits);
    let expected = expected.unwrap();
    assert!(expected.iter().filter(|d| d.len() >= 64 * 1024).count() > 4, "the archive has entries worth a thread");
    for threads in [1, 2, 7, 31] {
        let (got, work) = on_threads(&file, &limits, threads);
        assert_eq!(got.unwrap(), expected, "{threads} threads");
        assert_eq!(work, expected_work, "{threads} threads: the work of the inflate is the same");
    }
}

#[test]
fn an_archive_read_on_threads_goes_on_as_one_read_one_at_a_time_would_have() {
    // The total of the package is kept as it is: an entry inflated again is not counted twice, and one inflated after the many is held to what is left.
    let file = build(&items());
    let mut archive = Archive::read(&file, &Limits::default()).unwrap();
    let mut work = Work::default();
    let first: Vec<usize> = (0..10).collect();
    let many = archive.inflate_many(&first, 7, &mut work).unwrap();
    for (index, data) in first.iter().zip(&many) {
        assert_eq!(&archive.inflate(*index, &mut work).unwrap(), data, "entry {index} again");
    }
    let rest: Vec<usize> = (10..archive.entries().len()).collect();
    let more = archive.inflate_many(&rest, 7, &mut work).unwrap();
    assert_eq!(more.len(), rest.len());
}

#[test]
fn the_first_entry_in_the_order_that_fails_is_the_error_whichever_finished_first() {
    // Two entries are damaged (a checksum that is not the bytes'), the later one near the front of the work of the threads (a small one) and the earlier one behind a large one.
    let mut items = items();
    let early = 8;
    let late = 21;
    for at in [early, late] {
        items[at].crc ^= 1;
    }
    let file = build(&items);
    let limits = Limits::default();
    let (expected, expected_work) = one_at_a_time(&file, &limits);
    let expected = expected.unwrap_err();
    assert_eq!(expected.class(), "zip_corrupt");
    for threads in [1, 2, 7, 31] {
        let (got, work) = on_threads(&file, &limits, threads);
        assert_eq!(got.unwrap_err(), expected, "{threads} threads: the error is the earlier entry's");
        assert_eq!(work, expected_work, "{threads} threads: what the entries after the failure cost is not counted");
    }
}

#[test]
fn a_package_total_that_runs_out_in_the_bytes_is_refused_at_the_same_entry_at_any_number_of_threads() {
    // Three honest entries of 70,000 bytes, then one whose header says 5,000 and which holds 70,000: the headers add to less than the package total, and the bytes pass what is left of it
    // (40,000) at the fourth entry, whichever entries were inflated first. Small entries follow.
    let mut items = Vec::new();
    for k in 0..3usize {
        items.push(Item::deflated(&format!("p{k}.bin"), &bytes(70_000, k as u64)));
    }
    items.push(Item::deflated("p3.bin", &bytes(70_000, 3)).claiming_size(5_000));
    for k in 4..14usize {
        items.push(Item::deflated(&format!("p{k}.bin"), &bytes(100, k as u64)));
    }
    let file = build(&items);
    let limits = Limits { max_package_bytes: 250_000, max_part_bytes: 90_000, max_ratio: 1_000_000, ..Limits::default() };
    let (expected, expected_work) = one_at_a_time(&file, &limits);
    let expected = expected.unwrap_err();
    assert_eq!(expected.class(), "package_too_large", "{expected}");
    for threads in [1, 2, 7, 31] {
        let (got, work) = on_threads(&file, &limits, threads);
        assert_eq!(got.unwrap_err(), expected, "{threads} threads");
        assert_eq!(work, expected_work, "{threads} threads");
    }
}

#[test]
fn an_entry_that_holds_more_than_it_says_is_refused_as_what_it_is_at_any_number_of_threads() {
    // Entries of 70,000 bytes whose rows say 65,000, with a part cap of 66,000: the bytes pass the cap (`part_too_large`) before they are the size they say. The same entries under a cap
    // that leaves them room are damage (`zip_corrupt`: not the size they say). Each is the error of the first of them, at any number of threads.
    let items: Vec<Item> = (0..6usize).map(|k| Item::deflated(&format!("p{k}.bin"), &bytes(70_000, k as u64)).claiming_size(65_000)).collect();
    let file = build(&items);
    for (cap, class) in [(66_000u64, "part_too_large"), (90_000, "zip_corrupt")] {
        let limits = Limits { max_part_bytes: cap, max_ratio: 1_000_000, ..Limits::default() };
        let (expected, expected_work) = one_at_a_time(&file, &limits);
        let expected = expected.unwrap_err();
        assert_eq!(expected.class(), class, "{expected}");
        for threads in [1, 2, 7, 31] {
            let (got, work) = on_threads(&file, &limits, threads);
            assert_eq!(got.unwrap_err(), expected, "cap {cap}, {threads} threads");
            assert_eq!(work, expected_work, "cap {cap}, {threads} threads");
        }
    }
}

#[test]
fn an_entry_that_was_inflated_before_is_read_one_at_a_time_and_an_index_past_the_entries_is_damage() {
    let file = build(&items());
    let mut archive = Archive::read(&file, &Limits::default()).unwrap();
    let mut work = Work::default();
    let before = archive.inflate(3, &mut work).unwrap();
    let all: Vec<usize> = (0..archive.entries().len()).collect();
    let got = archive.inflate_many(&all, 7, &mut work).unwrap();
    assert_eq!(got.get(3), Some(&before));
    let error = archive.inflate_many(&[0, 9_999], 7, &mut work).unwrap_err();
    assert_eq!(error.class(), "zip_corrupt");
}

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

const THREADS: &str = "PAR_READ_THREADS";

/// The child of the test below: twelve entries that each **say they are 2 MiB and hold 200 MiB** (the headers pass every limit, and the bytes are far past what they say), inflated on
/// the threads `PAR_READ_THREADS` gives, under `ulimit -v 3000000`. Prints the error and the address space the process reached.
#[test]
#[ignore = "run by the test below in a process of its own, under an address-space limit"]
fn hostile_child() {
    let Ok(threads) = std::env::var(THREADS) else { return };
    let threads: usize = threads.parse().unwrap();
    let zeros = vec![0u8; 200 << 20];
    let one = Item::deflated("p.bin", &zeros).claiming_size(2 << 20);
    drop(zeros);
    let items: Vec<Item> = (0..12)
        .map(|k| {
            let mut item = one.clone();
            item.name = format!("p{k}.bin").into_bytes();
            item
        })
        .collect();
    drop(one);
    let file = build(&items);
    drop(items);
    let limits = Limits { max_part_bytes: 256 << 20, max_package_bytes: 1 << 30, max_ratio: 1_000_000_000, ..Limits::default() };
    let mut archive = Archive::read(&file, &limits).unwrap();
    let mut work = Work::default();
    let indices: Vec<usize> = (0..archive.entries().len()).collect();
    let error = archive.inflate_many(&indices, threads, &mut work).unwrap_err();
    println!("HOSTILE threads={threads} error={error} vm_peak_mib={}", status_kib("VmPeak") >> 10);
}

fn hostile(threads: usize) -> (String, u64) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact par_read::hostile_child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(THREADS, threads.to_string()).output().expect("run /bin/sh");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line = text
        .lines()
        .find_map(|l| l.find("HOSTILE ").map(|at| l[at..].to_owned()))
        .unwrap_or_else(|| panic!("the child ended with {:?}, and no line: {text}", out.status));
    let (head, peak) = line.rsplit_once(" vm_peak_mib=").unwrap();
    (head.split_once(" error=").unwrap().1.to_owned(), peak.trim().parse().unwrap())
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn entries_that_hold_far_more_than_they_say_are_refused_alike_at_one_and_at_many_threads_in_no_more_memory() {
    let (alone, alone_peak) = hostile(1);
    let (many, many_peak) = hostile(12);
    assert!(alone.contains("says it is 2097152 bytes"), "the one-at-a-time read refuses the first entry for what it holds: {alone}");
    assert_eq!(many, alone, "the same words at 12 threads as at 1");
    // The one-at-a-time read holds the one entry it is in the middle of (200 MiB); on threads no entry is read past the size it says, so the address space is the one-at-a-time read's
    // and the stacks and arenas of the threads, and not twelve entries of 200 MiB.
    assert!(many_peak < alone_peak + 12 * 90, "12 threads reached {many_peak} MiB of address space, 1 thread {alone_peak} MiB");
}

#[test]
fn a_read_that_goes_on_past_the_errors_it_tolerates_is_the_same_at_any_number_of_threads() {
    // Damaged entries (a checksum that is not the bytes', and one that holds more than its row says within the limits) are tolerated, as a caller that matches on `zip_corrupt` and goes on
    // does; one entry that passes the part cap is not, and ends the results. What the tolerated ones cost is counted, and they are no part of the package total.
    let mut items = items();
    for at in [2, 9] {
        items[at].crc ^= 1;
    }
    items[13] = Item::deflated("p13.bin", &bytes(150_000, 13)).claiming_size(70_000);
    items[26] = Item::deflated("p26.bin", &bytes(460_000, 26)).claiming_size(300_000);
    let file = build(&items);
    let limits = Limits { max_part_bytes: 450_000, max_ratio: 1_000_000, ..Limits::default() };
    let tolerate = |e: &ContainerError| e.class() == "zip_corrupt";
    let each = |threads: usize| {
        let mut archive = Archive::read(&file, &limits).unwrap();
        let mut work = Work::default();
        let indices: Vec<usize> = (0..archive.entries().len()).collect();
        let results = archive.inflate_each(&indices, threads, &tolerate, &mut work);
        // What the archive says afterwards: a read of the last entry that was not refused, held to what is left of the total.
        let again = archive.inflate(0, &mut Work::default()).map(|d| d.len());
        (results, work, again)
    };
    let (expected, expected_work, expected_again) = each(1);
    assert!(expected.iter().filter(|r| r.is_err()).count() >= 4, "damaged entries are in the results");
    assert!(expected.len() > 14, "the read went on past them");
    for threads in [2, 7, 31] {
        let (got, work, again) = each(threads);
        assert_eq!(got, expected, "{threads} threads");
        assert_eq!(work, expected_work, "{threads} threads");
        assert_eq!(again, expected_again, "{threads} threads");
    }
}

#[test]
fn entries_inflated_into_buffers_on_threads_are_the_same_as_one_at_a_time_and_the_archive_counts_nothing() {
    use diacraft_container::zip::IntoJob;
    use std::sync::Arc;
    let mut items = items();
    items[4].crc ^= 1;
    items[17].crc ^= 1;
    let file = build(&items);
    let limits = Limits::default();
    // One at a time: the bytes, or the error, and what each cost.
    let mut archive = Archive::read(&file, &limits).unwrap();
    let mut expected = Vec::new();
    for index in 0..archive.entries().len() {
        let size = usize::try_from(archive.entries()[index].size).unwrap();
        let mut out = vec![0u8; size];
        let mut work = Work::default();
        let result = archive.inflate_into(index, &mut out, &mut work).map(|()| out);
        expected.push((index, result, work));
    }
    assert!(expected.iter().filter(|(_, r, _)| r.is_err()).count() == 2);
    for threads in [2, 7, 31] {
        let archive = Archive::read(&file, &limits).unwrap();
        let jobs: Vec<IntoJob> = (0..archive.entries().len())
            .map(|index| {
                let size = usize::try_from(archive.entries()[index].size).unwrap();
                IntoJob { index, buffer: Arc::from(vec![0u8; size]) }
            })
            .collect();
        let done = archive.inflate_into_many(jobs, threads);
        assert_eq!(done.len(), expected.len(), "{threads} threads");
        for (got, (index, want, work)) in done.iter().zip(&expected) {
            assert_eq!(got.index, *index);
            assert_eq!(got.result.as_ref().map(|b| b.to_vec()).map_err(Clone::clone), *want, "entry {index} at {threads} threads");
            assert_eq!(got.work, *work, "entry {index} at {threads} threads");
        }
    }
}

#[test]
fn an_entry_counted_after_it_was_inflated_into_a_buffer_on_threads_is_counted_once() {
    use diacraft_container::zip::IntoJob;
    use std::sync::Arc;
    // A package total that has room for the entries once and not twice: counting an entry again does not take its size from the total again, as `inflate_into` does not.
    let content = bytes(100_000, 5);
    let file = build(&[Item::stored("a.bin", &content), Item::stored("b.bin", &content)]);
    let limits = Limits { max_package_bytes: 250_000, ..Limits::default() };
    let mut archive = Archive::read(&file, &limits).unwrap();
    let jobs = vec![IntoJob { index: 0, buffer: Arc::from(vec![0u8; 100_000]) }, IntoJob { index: 1, buffer: Arc::from(vec![0u8; 100_000]) }];
    for done in archive.inflate_into_many(jobs, 3) {
        assert!(done.result.is_ok());
    }
    archive.count_read(0);
    archive.count_read(0);
    archive.count_read(1);
    // 200,000 of 250,000 are counted: a third entry of 60,000 (of the same package) would not fit, and 50,000 would. Read again, an entry is not counted a second time.
    let mut again = vec![0u8; 100_000];
    assert!(archive.inflate_into(0, &mut again, &mut Work::default()).is_ok());
    assert_eq!(again, content);
}
