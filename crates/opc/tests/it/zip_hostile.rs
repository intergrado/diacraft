//! Hostile archives: not a zip, truncated, a bomb by its header and in fact, a header that lies, a method nobody supports, encryption,
//! entries that overlap, an end record that claims a billion entries, and a million real ones.

use std::time::{Duration, Instant};

use diacraft_opc::{Limits, OpcError, Package};

use crate::zipbuild::{Item, Opts, build, build_with, many_empty_entries};
use crate::{base_items, class_of, class_of_more, package_with};

/// A hang guard and no measure of cost: the work takes milliseconds to a second, and each test asserts what the read gave (an error of a
/// class, or the parts). A minute is generous for a machine at load 50, and a read that inflated the bomb would not end.
const QUICK: Duration = Duration::from_secs(60);

fn default_class(bytes: &[u8]) -> &'static str {
    class_of(bytes, &Limits::default())
}

#[test]
fn bytes_that_are_not_a_zip_are_not_zip() {
    assert_eq!(default_class(b""), "not_zip");
    assert_eq!(default_class(b"P"), "not_zip");
    assert_eq!(default_class(b"just some text, not an archive at all"), "not_zip");
    assert_eq!(default_class(b"<?xml version=\"1.0\"?><VisioDocument/>"), "not_zip", "an XML file under a package extension");
    assert_eq!(default_class(&[0u8; 4096]), "not_zip");
    assert_eq!(default_class(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), "not_zip");
    assert_eq!(default_class(&(0..=255u8).cycle().take(100_000).collect::<Vec<u8>>()), "not_zip");
}

#[test]
fn a_compound_file_is_named_and_not_opened_further() {
    // The compound file signature: old Office files, and encrypted packages (R22 `ole_with_package_extension`).
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend(std::iter::repeat_n(0u8, 2000));
    assert_eq!(default_class(&ole), "ole_with_package_extension");
    // Eight bytes and no more are still that file; seven are not.
    assert_eq!(default_class(&ole[..8]), "ole_with_package_extension");
    assert_eq!(default_class(&ole[..7]), "not_zip");
}

#[test]
fn a_zip_cut_short_is_corrupt_not_not_zip() {
    // The central directory is at the end, so any cut loses it. A file that starts like a zip and ends early is a damaged zip.
    let full = package_with(vec![Item::deflated("visio/document.xml", &b"<d/>".repeat(100)), Item::stored("a.bin", &[7u8; 300])]);
    for cut in (0..full.len()).step_by(7).chain([full.len() - 1, full.len() - 21, full.len() - 22, full.len() - 23]) {
        match Package::read(&full[..cut], &Limits::default()) {
            Ok(_) => panic!("a zip cut at {cut} of {} read", full.len()),
            Err(e) => {
                if cut >= 4 {
                    assert_eq!(e.class(), "zip_corrupt", "cut at {cut}");
                } else {
                    assert_eq!(e.class(), "not_zip", "cut at {cut}");
                }
            }
        }
    }
}

#[test]
fn a_zip_with_a_damaged_end_or_directory_is_corrupt() {
    let full = package_with(vec![Item::deflated("a.xml", b"<a/>")]);
    // The end record's offset of the central directory, set past the end of the file.
    let mut bad_offset = full.clone();
    let n = bad_offset.len();
    bad_offset[n - 6..n - 2].copy_from_slice(&0x7FFF_FFF0u32.to_le_bytes());
    assert_eq!(default_class(&bad_offset), "zip_corrupt");
    // The signature of the central directory entry destroyed.
    let mut bad_dir = full.clone();
    let at = bad_dir.windows(4).position(|w| w == [0x50, 0x4B, 0x01, 0x02]).unwrap();
    bad_dir[at] = 0;
    assert_eq!(default_class(&bad_dir), "zip_corrupt");
    // The end record claims more entries than the directory holds.
    let lie = build_with(&[base_items().remove(0)], &Opts { claimed_entries: Some(60_000), ..Opts::default() });
    assert_eq!(default_class(&lie), "zip_corrupt");
}

#[test]
fn a_part_whose_deflate_stream_is_damaged_is_corrupt() {
    let mut item = Item::deflated("a.xml", &b"<a/>".repeat(500));
    let mid = item.data.len() / 2;
    item.data[mid] ^= 0xFF;
    item.data[mid + 1] ^= 0xFF;
    assert_eq!(class_of_more(vec![item]), "zip_corrupt");
    // A stream that stops before it has ended.
    let mut short = Item::deflated("a.xml", &(0..20_000u32).flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>());
    short.data.truncate(short.data.len() / 2);
    short.csize = short.data.len() as u64;
    assert_eq!(class_of_more(vec![short]), "zip_corrupt");
}

#[test]
fn an_entry_with_no_data_at_all_is_an_empty_part_whichever_method_it_names() {
    // Some writers give an empty entry no stream and others give it the two bytes of an empty deflate stream; both are an empty
    // part, and an empty one's checksum is 0.
    let empty = |method: u16, data: &[u8]| Item { method, data: data.to_vec(), csize: data.len() as u64, ..Item::stored("e.xml", b"") };
    for item in [empty(0, b""), empty(8, b""), empty(8, &[0x03, 0x00])] {
        let p = Package::read(&package_with(vec![item]), &Limits::default()).unwrap();
        assert_eq!(p.part("e.xml"), Some(&b""[..]));
    }
    let mut wrong = empty(8, b"");
    wrong.crc = 1;
    assert_eq!(class_of_more(vec![wrong]), "zip_corrupt");
    // An entry that says it is empty and has data is not empty.
    assert_eq!(class_of_more(vec![Item::deflated("e.xml", b"not empty").claiming_size(0)]), "zip_corrupt");
}

#[test]
fn a_wrong_checksum_is_corrupt() {
    assert_eq!(class_of_more(vec![Item::stored("a.xml", b"<a/>").with_bad_crc()]), "zip_corrupt");
    assert_eq!(class_of_more(vec![Item::deflated("a.xml", b"<a/>").with_bad_crc()]), "zip_corrupt");
}

#[test]
fn a_header_that_lies_about_the_size_within_the_limits_is_corrupt() {
    // The size the header states is not the size of the data: the archive contradicts itself, whichever way.
    assert_eq!(class_of_more(vec![Item::deflated("a.xml", &[b'a'; 100]).claiming_size(50)]), "zip_corrupt");
    assert_eq!(class_of_more(vec![Item::deflated("a.xml", &[b'a'; 100]).claiming_size(150)]), "zip_corrupt");
    assert_eq!(class_of_more(vec![Item::stored("a.xml", &[b'a'; 100]).claiming_size(99)]), "zip_corrupt");
    // A compressed size that is not the stream's: too small cuts the stream, too large reaches past it.
    assert_eq!(class_of_more(vec![Item::deflated("a.xml", &[b'a'; 100]).claiming_csize(1)]), "zip_corrupt");
    assert_eq!(class_of_more(vec![Item::stored("a.xml", &[b'a'; 100]).claiming_csize(1 << 40)]), "zip_corrupt");
}

#[test]
fn a_header_that_claims_a_bomb_is_refused_by_its_ratio_before_anything_is_inflated() {
    // "an item declaring 1 MiB compressed and 2 GiB uncompressed" is `zip_ratio_exceeded`. The stream is a megabyte of
    // bytes that would not inflate at all: the refusal comes from the header, so it never gets that far.
    let junk: Vec<u8> = (0..1 << 20).map(|i: u32| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let item = Item::stored("visio/pages/page1.xml", &junk).claiming_size(2 << 30);
    assert_eq!(class_of_more(vec![item]), "zip_ratio_exceeded");
    let deflated = Item::deflated("a.xml", &junk).claiming_size(2 << 30);
    assert_eq!(class_of_more(vec![deflated]), "zip_ratio_exceeded");
    // Beyond four gigabytes the headers carry ZIP64 sizes, and they are read in full: no wrap to a small number.
    let huge = Item::stored("a.xml", &junk).claiming_size((1 << 40) + 5);
    assert_eq!(class_of_more(vec![huge]), "zip_ratio_exceeded");
}

#[test]
fn the_ratio_limit_holds_at_its_boundary_by_the_header() {
    // A header claiming exactly ratio x compressed passes the ratio check (and fails later, as corrupt, because the data is not
    // that long); one byte more is refused for its ratio.
    let c = 40u64;
    let data = vec![1u8; c as usize];
    let at = Item::stored("a.xml", &data).claiming_size(c * 10);
    let over = Item::stored("a.xml", &data).claiming_size(c * 10 + 1);
    let limits = Limits { max_ratio: 10, ..Limits::default() };
    assert_eq!(class_of(&package_with(vec![at]), &limits), "zip_corrupt");
    assert_eq!(class_of(&package_with(vec![over]), &limits), "zip_ratio_exceeded");
}

#[test]
fn the_ratio_limit_holds_by_the_bytes_a_part_really_holds() {
    // A real bomb: about 1,032 to 1. Over the cap of 1,000 by the header (the header is honest here).
    let n = 258 * 100_000 + 1;
    assert_eq!(class_of_more(vec![Item::zeros("a.xml", n)]), "zip_ratio_exceeded");
    // The same bytes with a limit above what they are, read in full, byte for byte.
    let roomy = Limits { max_ratio: 2000, ..Limits::default() };
    let p = Package::read(&package_with(vec![Item::zeros("a.xml", n)]), &roomy).unwrap();
    assert_eq!(p.part("a.xml").map(<[u8]>::len), Some(n as usize));
    assert!(p.part("a.xml").unwrap().iter().all(|b| *b == 0));
}

#[test]
fn a_header_that_lies_low_is_caught_from_the_bytes_that_come_out() {
    // V10, second sentence: "given a central directory that lies about the size, the same class is reported from actual
    // bytes". The header says 10 bytes, which passes every check on the header; the stream is a bomb.
    let n = 258 * 100_000 + 1;
    let lying = Item::zeros("a.xml", n).claiming_size(10);
    assert_eq!(class_of_more(vec![lying]), "zip_ratio_exceeded");
    // Lying low on an entry whose real ratio is within the limit is not a bomb, and not honest either: corrupt, as above.
    let lying_ok = Item::zeros("a.xml", 258 * 1000 + 1).claiming_size(10);
    let roomy = Limits { max_ratio: 2000, ..Limits::default() };
    assert_eq!(class_of(&package_with(vec![lying_ok]), &roomy), "zip_corrupt");
}

#[test]
fn a_header_that_lies_low_is_stopped_at_the_part_limit_when_that_is_the_nearer_one() {
    // The stream inflates past the per-part limit before it reaches the ratio limit: `part_too_large`, from the actual bytes.
    let n = 258 * 5_000 + 1; // 1,290,001 bytes
    let limits = Limits { max_part_bytes: 100_000, max_ratio: 5000, ..Limits::default() };
    assert_eq!(class_of(&package_with(vec![Item::zeros("a.xml", n).claiming_size(10)]), &limits), "part_too_large");
    // And honest, the header says so first.
    assert_eq!(class_of(&package_with(vec![Item::zeros("a.xml", n)]), &limits), "part_too_large");
}

#[test]
fn a_bomb_does_not_cost_more_to_refuse_than_the_ratio_allows() {
    // 100 MB of zeros from 100 KB, refused by the header: nothing is inflated, so this is fast whatever the machine does.
    let n = 258 * 400_000 + 1;
    let bytes = package_with(vec![Item::zeros("a.xml", n)]);
    assert!(bytes.len() < 200_000, "the archive is {} bytes", bytes.len());
    let start = Instant::now();
    assert_eq!(default_class(&bytes), "zip_ratio_exceeded");
    assert!(start.elapsed() < QUICK);
}

#[test]
fn unsupported_methods_are_refused_by_name_whatever_the_data() {
    // Only stored (0) and deflate (8). Shrink 1, implode 6, deflate64 9, bzip2 12, LZMA 14, zstd 93, WinZip AES 99.
    for method in [1u16, 2, 6, 9, 12, 14, 93, 95, 98, 99, 4242] {
        let item = Item::stored("a.xml", b"<a/>").with_method(method);
        assert_eq!(class_of_more(vec![item]), "unsupported_compression", "method {method}");
    }
    let err = Package::read(&package_with(vec![Item::stored("a.xml", b"<a/>").with_method(12)]), &Limits::default()).unwrap_err();
    assert!(err.to_string().contains("a.xml") && err.to_string().contains("12"), "{err}");
}

#[test]
fn an_encrypted_part_is_an_encrypted_package() {
    assert_eq!(class_of_more(vec![Item::stored("a.xml", b"<a/>").encrypted()]), "encrypted_package");
    // Whatever method it names: encryption is the first thing said.
    assert_eq!(class_of_more(vec![Item::stored("a.xml", b"<a/>").encrypted().with_method(99)]), "encrypted_package");
    assert_eq!(class_of_more(vec![Item::deflated("a.xml", b"<a/>").encrypted()]), "encrypted_package");
}

#[test]
fn entries_that_overlap_are_corrupt() {
    // The overlapping-entries bomb: many central directory rows share one stream, so the inflated total is a multiple of the
    // file's own size that no ratio per entry shows. The rows are refused when they overlap.
    let big = Item::zeros("big.bin", 258 * 1000 + 1);
    let mut items = base_items();
    items.push(big);
    for i in 0..5 {
        items.push(Item::zeros(&format!("alias{i}.bin"), 258 * 1000 + 1).aliasing(1));
    }
    let limits = Limits { max_ratio: 5000, ..Limits::default() };
    assert_eq!(class_of(&build(&items), &limits), "zip_corrupt");
}

#[test]
fn an_end_record_that_claims_a_billion_entries_is_damage_and_costs_nothing() {
    // ZIP64 end records with a count of 2^31 up to 2^64 - 1, in a file of a few hundred bytes: the rows cannot fit in the bytes the
    // directory has, so it is `zip_corrupt` (damage), found before a row is read, and not `too_many_parts`.
    for claimed in [1u64 << 31, 1 << 40, 1 << 63, u64::MAX] {
        let bytes = build_with(&[base_items().remove(0)], &Opts { claimed_entries: Some(claimed), zip64: true, ..Opts::default() });
        let start = Instant::now();
        assert_eq!(default_class(&bytes), "zip_corrupt", "claimed {claimed}");
        assert!(start.elapsed() < QUICK);
    }
}

#[test]
fn one_flipped_bit_in_a_count_is_damage_and_not_too_many_parts() {
    // Finding 13: a count with its high bit flipped used to read as `too_many_parts`. The count is checked against the bytes first. (The
    // count for this disk and the total are flipped together, so that it is the bytes that refuse it and not a disagreement between
    // them.)
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a/>"));
    let plain = build(&items);
    let at = plain.len() - 22; // no comment: the end record is the last 22 bytes
    let mut flipped = plain.clone();
    flipped[at + 9] |= 0x80;
    flipped[at + 11] |= 0x80; // 32,770 rows in a directory of two
    assert_eq!(default_class(&flipped), "zip_corrupt");
    // The same in the ZIP64 count (the 64-bit counts, at 24 and 32 into the ZIP64 record): bit 31, two billion rows.
    let wide = build_with(&items, &Opts { zip64: true, ..Opts::default() });
    let record = wide.windows(4).rposition(|w| w == b"PK\x06\x06").unwrap();
    let mut flipped = wide.clone();
    flipped[record + 24 + 3] |= 0x80;
    flipped[record + 32 + 3] |= 0x80;
    assert_eq!(default_class(&flipped), "zip_corrupt");
    // And the same flipped in the directory's size, which then holds the rows but runs past the end record.
    let mut size = plain.clone();
    size[at + 15] |= 0x80;
    assert_eq!(default_class(&size), "zip_corrupt");
    // Not only the high bits. Every bit of both plain counts, flipped (2 is `10`: bit 1 flipped is no rows, and the
    // others make 3, 6, 18, ...), and every low bit of the ZIP64 count, flipped alone and together with the other count.
    for bit in 0..16u32 {
        let mut count = plain.clone();
        for field in [at + 8, at + 10] {
            let value = u16::from_le_bytes([count[field], count[field + 1]]) ^ (1 << bit);
            count[field..field + 2].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(default_class(&count), "zip_corrupt", "plain counts, bit {bit}");
    }
    for bit in 0..32u32 {
        for fields in [&[24usize][..], &[24, 32][..]] {
            let mut count = wide.clone();
            for &field in fields {
                let at = record + field;
                let value = u64::from_le_bytes(count[at..at + 8].try_into().unwrap()) ^ (1 << bit);
                count[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            assert_eq!(default_class(&count), "zip_corrupt", "ZIP64 counts {fields:?}, bit {bit}");
        }
    }
}

#[test]
fn a_count_that_fits_but_is_past_the_limit_is_too_many_parts_before_any_row_is_read() {
    // A part limit of 10 lets 36 rows (twice the limit and sixteen). An honest directory of 36 rows reads; 37 do not, and a claim of
    // 5,000 rows that fit in the bytes (rows only, all of them the same and none with an entry behind it, so that reading one would
    // fail some other way) is refused for its count and for nothing else, before the first row.
    let limits = Limits { max_parts: 10, ..Limits::default() };
    let honest = |rows: usize| {
        let mut items = base_items();
        for i in 0..rows - 1 {
            items.push(Item::stored(&format!("d{i}/"), b""));
        }
        build(&items)
    };
    assert_eq!(Package::read(&honest(36), &limits).unwrap().parts().count(), 0);
    assert_eq!(class_of(&honest(37), &limits), "too_many_parts");
    assert_eq!(class_of(&crate::zipbuild::rows_only(5000, 0), &limits), "too_many_parts");
}

#[test]
fn an_honest_directory_of_many_items_is_not_refused_by_the_claim() {
    // Twice the part limit and sixteen is room for a good many directory items beside the parts: 20 of them, 10 parts and the content
    // types stream are 31 entries against the 36 that a part limit of 10 lets a claim be.
    let limits = Limits { max_parts: 10, ..Limits::default() };
    let mut items = base_items();
    for i in 0..20 {
        items.push(Item::stored(&format!("d{i}/"), b""));
    }
    for i in 0..10 {
        items.push(Item::stored(&format!("p{i}.bin"), b"x"));
    }
    assert_eq!(Package::read(&build(&items), &limits).unwrap().parts().count(), 10);
}

#[test]
fn a_million_entries_are_refused_as_too_many_parts() {
    // A million real entries (about 90 MB): one more than the cap by twenty, so this is the cap and not the format.
    let ct = crate::content_types_xml(&[("xml", "application/xml")], &[]);
    let bytes = many_empty_entries(1_000_000, ct.as_bytes());
    let start = Instant::now();
    assert_eq!(default_class(&bytes), "too_many_parts");
    assert!(start.elapsed() < QUICK, "took {:?}", start.elapsed());
}

#[test]
fn the_part_count_limit_holds_at_its_boundary_and_does_not_count_the_content_types_or_directories() {
    let limits = Limits { max_parts: 3, ..Limits::default() };
    let mk = |n: usize, dirs: usize| {
        let mut items = base_items();
        for i in 0..dirs {
            items.push(Item::stored(&format!("d{i}/"), b""));
        }
        for i in 0..n {
            items.push(Item::stored(&format!("p{i}.bin"), b"x"));
        }
        build(&items)
    };
    assert_eq!(Package::read(&mk(3, 0), &limits).unwrap().parts().count(), 3);
    assert_eq!(Package::read(&mk(3, 5), &limits).unwrap().parts().count(), 3, "directory items are not parts");
    assert_eq!(class_of(&mk(4, 0), &limits), "too_many_parts");
    assert_eq!(class_of(&mk(4, 5), &limits), "too_many_parts");
}

#[test]
fn names_that_come_out_are_canonical_and_names_that_would_leave_the_package_do_not_come_out() {
    // Review finding 14: this test asserted almost nothing. No test can watch for a file that is not opened (this crate opens none: its
    // names are keys into its own map), so what is pinned is what could matter to something that does open files, a writer or another
    // reader: every name that comes out of a read is a part name as R3 has it, whatever odd but legal thing the item was, and every item
    // that names a place outside the package is an error and comes out of nothing.
    let odd = [
        "a b.xml",
        "dir with spaces/file.xml",
        ".hidden",
        "visio/.hidden.xml",
        "a..b.xml",
        "a...b",
        "-dash",
        "~tilde",
        "100%.xml",
        "a%zzb",
        "caf%C3%A9.xml",
        "\u{e9}\u{4e2d}\u{6587}/\u{1f600}.xml",
        "UPPER/Lower/MiXed.XML",
        "a/b/c/d/e/f/g/h/i/j/k.xml",
        "x",
        "name.with.many.dots.xml",
        "[brackets].xml",
        "a(b).xml",
        "a;b.xml",
        "a,b.xml",
        "a=b.xml",
        "a@b.xml",
        "a&b.xml",
        "a'b.xml",
        "a!b.xml",
        "a$b.xml",
    ];
    let mut items = base_items();
    items.extend(odd.iter().map(|n| Item::stored(n, b"1")));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    let names: Vec<&str> = p.parts().collect();
    assert_eq!(names.len(), odd.len());
    for name in &names {
        let segments: Vec<&str> = name.strip_prefix('/').expect("a leading slash").split('/').collect();
        assert!(segments.iter().all(|s| !s.is_empty() && !s.ends_with('.')), "{name}");
        assert!(!name.contains('\\') && !name.chars().any(char::is_control), "{name:?}");
        assert!(p.part(name).is_some(), "{name}");
    }
    // Each of these names a place outside the package or smuggles a separator past a check; none is a part and none comes out.
    let leaving =
        ["../x", "a/../../x", "/abs", "//unc/x", "C:/x", "c:x", "a\\b", "..\\x", "%2e%2e/x", "a%2f..%2fb", "a%5c..", "./../x", "a/./../../x", "a/.."];
    for name in leaving {
        let err = Package::read(&package_with(vec![Item::stored(name, b"1")]), &Limits::default()).unwrap_err();
        assert!(matches!(err.class(), "path_traversal" | "invalid_part_name"), "{name:?}: {err}");
    }
}

#[test]
fn error_text_cannot_carry_control_characters_from_a_hostile_name() {
    let err: OpcError =
        Package::read(&package_with(vec![Item::stored("x", b"1").named_bytes(b"../\x1b[31mred\r\n\x07.xml")]), &Limits::default()).unwrap_err();
    let text = err.to_string();
    assert!(!text.chars().any(|c| c.is_control()), "{text:?}");
    assert!(text.contains("\\u{1b}") && text.contains("red"), "the name is in the message, escaped: {text:?}");
}
