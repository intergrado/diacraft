//! **A package is the same bytes at any number of threads.** [`ArchiveWriter::add_many`] compresses entries on workers and writes them in the order given, and what it
//! writes is what adding the entries one after another writes: byte for byte, at 1, 2, 7 and 31 threads, over entries of every [`Pack`], of every size from none to past the
//! fast level, in runs that are the same each time, and under a window that lets one entry in flight or many. A refusal is the first of the sequence, in its place.

use diacraft_container::par::{self, Lane};
use diacraft_container::zip::{Add, Archive, ArchiveWriter, Method, Pack, Workers, pack};
use diacraft_container::{ContainerError, Limits, Work};

/// Bytes that deflate hardly at all.
fn noise(len: usize, mut state: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// Text of the kind a part of a drawing is: markup that repeats with numbers that do not.
fn markup(len: usize, seed: u64) -> Vec<u8> {
    let mut out = String::with_capacity(len + 200);
    let mut state = seed;
    while out.len() < len {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let n = state >> 40;
        out.push_str(&format!(
            "<Shape ID=\"{}\" Type=\"Shape\"><Cell N=\"PinX\" V=\"{}.{}\"/><Cell N=\"Width\" V=\"1\" F=\"Height*2\"/></Shape>\r\n",
            n % 9999,
            n % 77,
            n % 13
        ));
    }
    out.truncate(len);
    out.into_bytes()
}

/// Entries of every kind: each way of packing, an empty one, a small one, ones that shrink and ones that do not, and one past the fast level's 4 MiB.
fn entries() -> Vec<(String, Vec<u8>, Pack)> {
    let mut v: Vec<(String, Vec<u8>, Pack)> = vec![
        ("mimetype".to_owned(), b"application/x-test".to_vec(), Pack::Stored),
        ("empty.bin".to_owned(), Vec::new(), Pack::Smallest),
        ("empty2.xml".to_owned(), Vec::new(), Pack::Deflated),
        ("small.xml".to_owned(), markup(300, 1), Pack::Smallest),
    ];
    for k in 0..4u64 {
        v.push((format!("pages/page{k}.xml"), markup(200_000 + 37_000 * k as usize, k + 10), Pack::Smallest));
    }
    v.push(("media/picture.bin".to_owned(), noise(1_500_000, 7), Pack::Smallest));
    v.push(("media/picture.png".to_owned(), noise(900_000, 8), Pack::Stored));
    v.push(("pattern.dat".to_owned(), (0..700_000usize).map(|i| (i % 251) as u8).collect(), Pack::Deflated));
    v.push(("level1.xml".to_owned(), markup(400_000, 21), Pack::Level(1)));
    v.push(("level6.xml".to_owned(), markup(400_000, 22), Pack::Level(6)));
    v.push(("level9.xml".to_owned(), markup(400_000, 23), Pack::Level(9)));
    v.push(("noise-deflated.bin".to_owned(), noise(100_000, 9), Pack::Deflated));
    v.push(("big.xml".to_owned(), markup(4_600_000, 31), Pack::Deflated));
    v.push(("tail.xml".to_owned(), markup(64_000, 41), Pack::Smallest));
    v.push(("last.txt".to_owned(), b"the last".to_vec(), Pack::Smallest));
    v
}

fn items(entries: &[(String, Vec<u8>, Pack)]) -> Vec<Add<'_>> {
    entries.iter().map(|(name, data, how)| Add { name, data, how: *how }).collect()
}

/// The archive that adding the entries one after another makes.
fn one_after_another(entries: &[(String, Vec<u8>, Pack)]) -> (Vec<u8>, Vec<Method>) {
    let mut writer = ArchiveWriter::new();
    let mut methods = Vec::new();
    for (name, data, how) in entries {
        let method = match how {
            Pack::Stored => writer.add(name, data, Method::Stored).map(|()| Method::Stored),
            Pack::Deflated => writer.add(name, data, Method::Deflated).map(|()| Method::Deflated),
            Pack::Smallest => writer.add_smallest(name, data),
            Pack::Level(level) => writer.add_at_level(name, data, *level).map(|()| Method::Deflated),
        };
        methods.push(method.expect("an entry is added"));
    }
    (writer.finish().expect("an archive"), methods)
}

fn many(entries: &[(String, Vec<u8>, Pack)], workers: Workers) -> (Vec<u8>, Vec<Method>) {
    let mut writer = ArchiveWriter::new();
    let methods = writer.add_many(&items(entries), workers).expect("the entries are added");
    (writer.finish().expect("an archive"), methods)
}

#[test]
fn the_archive_is_byte_for_byte_the_one_entries_added_one_after_another_make_at_any_number_of_threads() {
    let entries = entries();
    let (expected, methods) = one_after_another(&entries);
    // The kinds are what the test says they are: some entries shrank, some were stored for want of shrinking, and the stored ones stayed stored.
    assert!(methods.contains(&Method::Stored) && methods.contains(&Method::Deflated));
    for threads in [1, 2, 7, 31] {
        let (bytes, got) = many(&entries, Workers::new(threads));
        assert_eq!(got, methods, "{threads} threads: the methods");
        assert!(
            bytes == expected,
            "{threads} threads: the archive differs from the single-threaded one ({} and {} bytes)",
            bytes.len(),
            expected.len()
        );
    }
}

#[test]
fn a_package_is_the_same_bytes_every_time_it_is_made_on_threads() {
    let entries = entries();
    let first = many(&entries, Workers::new(7)).0;
    for run in 0..6 {
        assert!(many(&entries, Workers::new(7)).0 == first, "run {run}");
    }
}

#[test]
fn the_window_decides_how_many_are_in_flight_and_never_what_is_written() {
    let entries = entries();
    let (expected, _) = one_after_another(&entries);
    // One entry at a time (a window of one byte: each is alone in flight), a few at a time, and all of them.
    for window in [1usize, 1_000_000, 5_000_000, 20_000_000, usize::MAX] {
        for threads in [2, 31] {
            let (bytes, _) = many(&entries, Workers { threads, window });
            assert!(bytes == expected, "{threads} threads, a window of {window}");
        }
    }
}

#[test]
fn the_archive_reads_back_with_every_entry_as_it_went_in() {
    let entries = entries();
    let (bytes, _) = many(&entries, Workers::new(31));
    let mut archive =
        Archive::read(&bytes, &Limits { max_part_bytes: 64 << 20, max_package_bytes: 256 << 20, ..Limits::default() }).expect("the archive reads");
    assert_eq!(archive.entries().len(), entries.len());
    for (i, (name, data, _)) in entries.iter().enumerate() {
        assert_eq!(&archive.entries().get(i).expect("an entry").name, name, "the order is the order given");
        let inflated = archive.inflate(i, &mut Work::default()).expect("the entry inflates");
        assert!(&inflated == data, "{name}");
    }
}

#[test]
fn a_refusal_is_the_first_of_the_sequence_in_its_place_and_what_came_before_it_is_written() {
    let mut entries = entries();
    // A name that leaves the package, in the middle; and a second refusal after it, which must never be the one that is reported.
    entries.insert(9, ("../outside.xml".to_owned(), markup(100_000, 3), Pack::Smallest));
    entries.insert(12, ("pages/page0.xml".to_owned(), markup(100_000, 4), Pack::Smallest));
    let mut alone = ArchiveWriter::new();
    let mut expected = None;
    for (name, data, how) in &entries {
        let r = match how {
            Pack::Stored => alone.add(name, data, Method::Stored),
            Pack::Deflated => alone.add(name, data, Method::Deflated),
            Pack::Smallest => alone.add_smallest(name, data).map(|_| ()),
            Pack::Level(l) => alone.add_at_level(name, data, *l),
        };
        if let Err(e) = r {
            expected = Some(e);
            break;
        }
    }
    let expected = expected.expect("the name is refused");
    assert_eq!(expected.class(), "path_traversal");
    for threads in [1, 2, 31] {
        let mut writer = ArchiveWriter::new();
        let got = writer.add_many(&items(&entries), Workers::new(threads)).expect_err("the name is refused");
        assert_eq!(got, expected, "{threads} threads");
        assert_eq!(writer.len(), alone.len(), "{threads} threads: the entries before it are written, and none after it");
    }
    // A level past 9 and a duplicate name are the writer's own: refused as they are one after another.
    let mut entries = self::entries();
    entries.insert(4, ("level.xml".to_owned(), markup(100_000, 5), Pack::Level(10)));
    for threads in [1, 31] {
        let got = ArchiveWriter::new().add_many(&items(&entries), Workers::new(threads)).expect_err("a level of 10 is refused");
        assert_eq!(got.class(), "unsupported_compression", "{threads} threads");
    }
    let mut entries = self::entries();
    entries.push(("pages/page1.xml".to_owned(), markup(40_000, 6), Pack::Smallest));
    for threads in [1, 31] {
        let mut writer = ArchiveWriter::new();
        writer.add_many(&items(&entries), Workers::new(threads)).expect("the names are checked at the end");
        assert_eq!(writer.finish().expect_err("two entries of a name").class(), "duplicate_part", "{threads} threads");
    }
}

#[test]
fn an_entry_compressed_apart_is_written_as_if_it_had_been_compressed_where_it_was_written() {
    let data = markup(300_000, 77);
    let packed = pack("a.xml", &data, Pack::Smallest);
    let mut by_parts = ArchiveWriter::new();
    let method = by_parts.add_packed("a.xml", &data, Pack::Smallest, packed).expect("written");
    let mut at_once = ArchiveWriter::new();
    assert_eq!(at_once.add_smallest("a.xml", &data).expect("written"), method);
    assert!(by_parts.finish().expect("archive") == at_once.finish().expect("archive"));
    // A refusal made apart is the refusal of the write.
    let refused = pack("/a.xml", &data, Pack::Smallest);
    assert!(matches!(refused, Err(ContainerError::PathTraversal { .. })));
}

#[test]
fn a_lane_of_the_container_gives_results_in_order_to_a_caller_that_writes_what_it_takes() {
    // The same promise at the level of the lane the writer is made of: the caller's thread writes in the order given while workers finish in any order.
    let data: Vec<Vec<u8>> = (0..12u64).map(|k| markup(60_000 + 5_000 * (k as usize % 4), k)).collect();
    let packed = par::lane(5, |lane: &Lane<'_, '_, usize>| {
        for (k, d) in data.iter().enumerate() {
            lane.submit(d.len(), move || {
                let p = pack("x.xml", d, Pack::Smallest).expect("packed");
                if p.method() == Method::Deflated { k } else { usize::MAX }
            });
        }
        let mut order = Vec::new();
        while let Some(r) = lane.take() {
            order.push(r.expect("a task"));
        }
        order
    });
    assert_eq!(packed, (0..12).collect::<Vec<_>>());
}
