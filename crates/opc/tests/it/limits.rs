//! The limits: the spec's numbers are the defaults, each holds at its boundary, and together they bound the whole package and
//! not only each part.

use diacraft_opc::{Limits, MAX_PACKAGE_BYTES, MAX_PART_BYTES, MAX_PARTS, MAX_RATIO, MAX_XML_DEPTH, Package, XmlLimits};

use crate::zipbuild::{Item, build};
use crate::{base_items, class_of, package_with};

#[test]
fn the_defaults_are_the_numbers_of_and_ruling_6() {
    // 50,000 parts, 256 MiB in a part, ratio 1000, depth 200; and 1 GiB in a package, which is
    // not the 8 GiB: lowers it until R23 is amended (the largest real package is 173 MB; 8 GiB let 8 MB ask for 8 GiB).
    let l = Limits::default();
    assert_eq!(l.max_parts, 50_000);
    assert_eq!(l.max_package_bytes, 1024 * 1024 * 1024);
    assert_eq!(l.max_part_bytes, 256 * 1024 * 1024);
    assert_eq!(l.max_ratio, 1000);
    assert_eq!(l.xml.max_depth, 200);
    // Each is a named constant, and the default is that constant.
    assert_eq!(
        (MAX_PARTS, MAX_PACKAGE_BYTES, MAX_PART_BYTES, MAX_RATIO, MAX_XML_DEPTH),
        (l.max_parts, l.max_package_bytes, l.max_part_bytes, l.max_ratio, l.xml.max_depth)
    );
    assert_eq!(XmlLimits::default(), l.xml);
}

#[test]
fn the_xml_and_package_caps_the_sample_file_run_set_are_the_rulings_numbers() {
    // And 25, each beside its real maximum: 1,481,648 items in a tree, an attribute of 43,350 bytes, `[Content_Types].xml` of
    // 190,513 and a relationships part of 221,863.
    let l = Limits::default();
    assert_eq!(l.xml.max_nodes, 6_000_000);
    assert_eq!(l.xml.max_attr_bytes, 1024 * 1024);
    assert_eq!(l.max_package_xml_bytes, 4 * 1024 * 1024);
    assert_eq!(
        (diacraft_opc::MAX_XML_NODES, diacraft_opc::MAX_XML_ATTR_BYTES, diacraft_opc::MAX_PACKAGE_XML_BYTES),
        (6_000_000, 1024 * 1024, 4 * 1024 * 1024)
    );
}

#[test]
fn the_size_of_a_part_is_bounded_at_the_limit_and_one_past_it() {
    let limits = Limits { max_part_bytes: 1000, ..Limits::default() };
    for make in [Item::stored as fn(&str, &[u8]) -> Item, Item::deflated] {
        let at = package_with(vec![make("a.bin", &[7u8; 1000])]);
        assert_eq!(Package::read(&at, &limits).unwrap().part("a.bin").map(<[u8]>::len), Some(1000));
        let over = package_with(vec![make("a.bin", &[7u8; 1001])]);
        assert_eq!(class_of(&over, &limits), "part_too_large");
    }
}

#[test]
fn the_size_of_a_part_that_the_header_understates_is_bounded_by_the_bytes() {
    // The header says 100 and the stream holds 5,000: stopped at the limit, not read to the end and compared afterwards.
    let limits = Limits { max_part_bytes: 1000, max_ratio: 1_000_000, ..Limits::default() };
    let item = Item::zeros("a.bin", 5000).claiming_size(100);
    assert_eq!(class_of(&package_with(vec![item]), &limits), "part_too_large");
}

#[test]
fn the_package_total_is_bounded_by_the_headers_before_a_byte_is_inflated() {
    // Three parts, each under the per-part limit (600 of 1,000), 1,800 together against 1,500. The first part's checksum is wrong:
    // if the inflating came first, the error would be `zip_corrupt`, from that part. It is the package that is refused, from the
    // headers, before anything is read.
    let limits = Limits { max_part_bytes: 1000, max_package_bytes: 1500, ..Limits::default() };
    let bytes = package_with(vec![Item::stored("a", &[1u8; 600]).with_bad_crc(), Item::stored("b", &[2u8; 600]), Item::stored("c", &[3u8; 600])]);
    assert_eq!(class_of(&bytes, &limits), "package_too_large");
    // With the headers within the limit, the same bad checksum is what is reported.
    let fits = package_with(vec![Item::stored("a", &[1u8; 600]).with_bad_crc(), Item::stored("b", &[2u8; 600])]);
    assert_eq!(class_of(&fits, &Limits { max_package_bytes: 1500, ..limits }), "zip_corrupt");
}

#[test]
fn the_package_total_holds_at_its_boundary() {
    // The content types stream is not counted: it is the reader's own, and the part limits are about parts.
    let limits = Limits { max_part_bytes: 1000, max_package_bytes: 1200, ..Limits::default() };
    let at = package_with(vec![Item::stored("a", &[1u8; 600]), Item::deflated("b", &[2u8; 600])]);
    assert_eq!(Package::read(&at, &limits).unwrap().parts().count(), 2);
    let over = package_with(vec![Item::stored("a", &[1u8; 600]), Item::deflated("b", &[2u8; 601])]);
    assert_eq!(class_of(&over, &limits), "package_too_large");
}

#[test]
fn the_package_total_is_bounded_by_the_bytes_when_a_header_understates() {
    // The first part is honest, 600 bytes. The second says 10 and holds 600, which passes every check of the headers, and the
    // total of the bytes is 1,200 against 1,000: stopped at what is left, and called what it is.
    let limits = Limits { max_part_bytes: 1000, max_package_bytes: 1000, max_ratio: 1_000_000, ..Limits::default() };
    let bytes = package_with(vec![Item::stored("a", &[1u8; 600]), Item::zeros("b", 600).claiming_size(10)]);
    assert_eq!(class_of(&bytes, &limits), "package_too_large");
}

#[test]
fn a_part_that_is_all_of_what_is_left_of_the_total_is_read() {
    let limits = Limits { max_part_bytes: 1000, max_package_bytes: 1000, ..Limits::default() };
    let p = Package::read(&package_with(vec![Item::stored("a", &[1u8; 400]), Item::stored("b", &[2u8; 600])]), &limits).unwrap();
    assert_eq!(p.part("b").map(<[u8]>::len), Some(600));
}

#[test]
fn many_parts_each_under_every_per_part_limit_are_bounded_as_a_whole() {
    // The trap the review looks for: a limit per part that does not bound the file. 400 parts of 99 bytes are each far under
    // a per-part limit of 100, and 39,600 bytes against a total of 10,000.
    let limits = Limits { max_part_bytes: 100, max_package_bytes: 10_000, ..Limits::default() };
    let mut items = base_items();
    for i in 0..400 {
        items.push(Item::deflated(&format!("p{i}"), &[b'x'; 99]));
    }
    assert_eq!(class_of(&build(&items), &limits), "package_too_large");
    // 101 of them are 9,999 bytes, within it.
    let mut items = base_items();
    for i in 0..101 {
        items.push(Item::deflated(&format!("p{i}"), &[b'x'; 99]));
    }
    assert_eq!(Package::read(&build(&items), &limits).unwrap().parts().count(), 101);
}

#[test]
fn the_reader_stops_at_the_first_refusal_and_keeps_nothing() {
    // A refusal is an error and no package: there is no half-read result to misuse.
    let limits = Limits { max_part_bytes: 100, ..Limits::default() };
    let bytes = package_with(vec![Item::stored("a", &[1u8; 50]), Item::stored("b", &[1u8; 101]), Item::stored("c", &[1u8; 50])]);
    assert_eq!(class_of(&bytes, &limits), "part_too_large");
}

#[test]
fn the_checks_of_the_headers_come_before_any_inflating_and_the_names_come_first() {
    // The last part has a wrong checksum, the first a traversal name: the name is what is reported.
    let bytes = package_with(vec![Item::stored("a/../b", b"1"), Item::stored("c", b"2").with_bad_crc()]);
    assert_eq!(class_of(&bytes, &Limits::default()), "path_traversal");
    // And an unsupported method before a bad checksum.
    let bytes = package_with(vec![Item::stored("c", b"2").with_bad_crc(), Item::stored("d", b"2").with_method(12)]);
    assert_eq!(class_of(&bytes, &Limits::default()), "unsupported_compression");
}

// ---- the package cap, on every platform (review M5)
//
// `memory.rs` reads four parts of 256 MiB and refuses a fifth under an address-space limit, which only Linux can set, so on the other
// systems that file is not compiled. What the cap *does* is tested here with small numbers, and what it *is* with its constant.

#[test]
fn four_parts_of_the_part_limit_read_a_fifth_is_package_too_large_and_a_larger_limit_opens_all_five() {
    // The shape of at 1 KiB and not 256 MiB: a part limit of 1,024 bytes and a package limit of four of them.
    let limits = Limits { max_part_bytes: 1024, max_package_bytes: 4 * 1024, ..Limits::default() };
    let part = |i: u8| Item::stored(&format!("p{i}.bin"), &[i; 1024]);
    let four = package_with((0..4).map(part).collect());
    let p = Package::read(&four, &limits).unwrap();
    assert_eq!(p.parts().filter(|n| p.part(n).map(<[u8]>::len) == Some(1024)).count(), 4);
    let five = package_with((0..5).map(part).collect());
    assert_eq!(class_of(&five, &limits), "package_too_large");
    let roomy = Limits { max_package_bytes: 5 * 1024, ..limits };
    assert_eq!(Package::read(&five, &roomy).unwrap().parts().count(), 5);
}

#[test]
fn the_package_cap_is_one_gibibyte_and_four_parts_of_the_part_limit() {
    // Until R23 is amended: not the 8 GiB.
    assert_eq!(MAX_PACKAGE_BYTES, 1 << 30);
    assert_eq!(MAX_PACKAGE_BYTES, 4 * MAX_PART_BYTES);
    assert_eq!(Limits::default().max_package_bytes, 1 << 30);
}

#[test]
fn five_parts_of_the_part_limit_are_refused_by_default_from_their_headers_on_every_platform() {
    // Five parts of 256 MiB (a file of 3 MB, at a ratio of 518) are 1.25 GiB against the default 1 GiB, and are refused from the headers,
    // before a byte of them is inflated: so the refusal costs nothing, and this test needs no gigabyte.
    let parts: Vec<Item> = (0..5).map(|i| Item::zeros_under_the_ratio_cap(&format!("p{i}.bin"), MAX_PART_BYTES)).collect();
    assert_eq!(class_of(&package_with(parts), &Limits::default()), "package_too_large");
}
