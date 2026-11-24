//! `cargo fuzz run native_read`: the `.diacraft` reader on any bytes. A read gives an error, in words and with a class, or a document that **holds the model's caps**
//! (`check_limits`), that **writes** (a refusal for a limit is in words and is not a failure; any other refusal is a crash), and that **reads back equal** to the model it was
//! written from; and the write of what was read back is a fixed point: its bytes are the bytes of the write after it. Anything else, a panic, a hang or a model the format cannot
//! give back, is a crash the fuzzer reports. **A read on threads is the read on one**. The limits are those of a small machine, so that an input stays quick: a package of 2 MiB, a tree of 200,000 items, 256 MiB of memory.
#![no_main]

use diacraft_native::{Limits, WriteOptions, read, sniff, write};
use libfuzzer_sys::fuzz_target;

fn small() -> Limits {
    let mut limits = Limits { max_file_bytes: 4 << 20, max_memory: 256 << 20, ..Limits::default() };
    limits.container.max_package_bytes = 2 << 20;
    limits.container.xml.max_nodes = 200_000;
    limits
}

/// The classes of a refusal for a limit: a write may end in one of them and be right.
const LIMITS: &[&str] = &[
    "too_large",
    "model_too_complex",
    "package_too_large",
    "part_too_large",
    "too_many_parts",
    "xml_too_complex",
    "zip_ratio_exceeded",
    "out_of_memory",
];

fuzz_target!(|data: &[u8]| {
    let limits = small();
    let _ = sniff(data);
    // **A read on threads is the read on one**: the same model, notes and cost, or the same refusal, with the pictures inflated ahead on threads.
    let threads = 2 + data.len() % 4;
    match (read(data, &limits), read(data, &Limits { threads, ..limits.clone() })) {
        (Ok((one, a)), Ok((many, b))) => {
            assert!(one == many && a.notes == b.notes && a.work == b.work, "a read on {threads} threads is not the read on one")
        }
        (Err(a), Err(b)) => {
            assert!(a.to_string() == b.to_string() && a.class() == b.class(), "a refusal on {threads} threads is not the refusal on one")
        }
        _ => panic!("a read on {threads} threads and a read on one disagree on whether the file reads"),
    }
    let (doc, opened) = match read(data, &limits) {
        Ok(read) => read,
        Err(e) => {
            assert!(!e.to_string().is_empty() && !e.class().is_empty(), "a refusal has words and a class");
            return;
        }
    };
    assert!(doc.check_limits().is_ok(), "a model that was read holds the model's caps");
    let options = WriteOptions { limits: limits.clone(), ..WriteOptions::default() };
    let first = match write(&doc, &opened.state, None, &options) {
        Ok(written) => written,
        Err(e) => {
            assert!(LIMITS.contains(&e.class()), "a model that was read is written, or refused for a limit: {e}");
            return;
        }
    };
    let (back, again) = match read(&first.bytes, &limits) {
        Ok(read) => read,
        // A package a write made is a package a read of the same limits takes, or a limit of the read's own that the write does not see.
        Err(e) => {
            assert!(LIMITS.contains(&e.class()), "what a write made is read: {e}");
            return;
        }
    };
    assert!(back == doc, "a model read, written and read is the model");
    let Ok(second) = write(&back, &again.state, None, &options) else { return };
    let Ok((twice, third)) = read(&second.bytes, &limits) else { return };
    assert!(twice == doc, "and reads back equal again");
    let Ok(last) = write(&twice, &third.state, None, &options) else { return };
    assert!(last.bytes == second.bytes, "the write of a model read back is a fixed point");
});
