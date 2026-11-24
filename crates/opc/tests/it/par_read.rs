//! **A package read on threads is the package read on one** (`Package::read_threads`): the same parts and warnings, and for a file with a fault the same error, at any number of threads.

use diacraft_opc::{Limits, Package};

use crate::content_types_xml;
use crate::zipbuild::{Item, build};

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

fn items(bad: Option<usize>) -> Vec<Item> {
    let types = content_types_xml(&[("xml", "application/xml"), ("bin", "application/octet-stream")], &[]);
    let mut items = vec![Item::deflated("[Content_Types].xml", types.as_bytes())];
    for k in 0..20usize {
        let content = bytes(if k % 3 == 0 { 300_000 } else { 2_000 }, k as u64 + 1);
        let item = Item::deflated(&format!("visio/p{k}.bin"), &content);
        items.push(if bad == Some(k) { item.with_bad_crc() } else { item });
    }
    items.push(Item::stored("visio/folder/", b""));
    items
}

#[test]
fn the_parts_and_warnings_are_the_same_at_any_number_of_threads() {
    let file = build(&items(None));
    let one = Package::read(&file, &Limits::default()).unwrap();
    for threads in [1, 2, 7, 31] {
        let many = Package::read_threads(&file, &Limits::default(), threads).unwrap();
        assert_eq!(many.parts().collect::<Vec<_>>(), one.parts().collect::<Vec<_>>(), "{threads} threads");
        for part in one.parts() {
            assert_eq!(many.part(part), one.part(part), "{part} at {threads} threads");
        }
        assert_eq!(many.warnings(), one.warnings(), "{threads} threads");
        assert_eq!(many.content_types_bytes(), one.content_types_bytes(), "{threads} threads");
    }
}

#[test]
fn a_damaged_entry_is_the_same_error_at_any_number_of_threads() {
    // Two parts have a checksum that is not their bytes': the error is the first in the directory.
    let mut damaged = items(Some(14));
    damaged[3] = Item::deflated("visio/p2.bin", &bytes(2_000, 3)).with_bad_crc();
    let file = build(&damaged);
    let expected = Package::read(&file, &Limits::default()).expect_err("a damaged part refuses the package");
    assert_eq!(expected.class(), "zip_corrupt");
    for threads in [1, 2, 7, 31] {
        let got = Package::read_threads(&file, &Limits::default(), threads).expect_err("refused");
        assert_eq!(got.to_string(), expected.to_string(), "{threads} threads");
    }
}
