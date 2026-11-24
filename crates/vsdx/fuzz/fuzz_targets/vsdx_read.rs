//! `cargo fuzz run vsdx_read`: the package reader on any input, with the budgets of a real read (`Limits::default()`).
//!
//! The input is a recipe (`diacraft_vsdx_fuzz::inputs`): the bytes as they are for one input in eight, else one part of a seed package (the plain one, or the one that carries the data of the
//! other format) replaced, or spliced into, and the package packed again with right checksums, so that the fuzzer explores the reader and not only the archive. A read gives a document or a
//! refusal in words, and **never panics, aborts or runs past the fuzzer's timeout**; the properties of a document that read are:
//!
//! * it passes the model's own caps, and the memory the read held stays within its pool;
//! * the bytes of each held part are the package's exactly, and the parts of the other format that DiaCraft's solution part carries are sound (no macro part in a type that allows none, nothing
//!   outside `/diacraft/` taken as a blob, names an `.odg` entry may have);
//! * a read of the same bytes twice gives the same document and report (for the inputs whose length is a multiple of 4);
//! * a read in a pool of any size beside the file is a read or a refusal for memory in words, and every tree it made was held in the pool (when the input asks for one);
//! * **a read on threads is the read on one**: the same document and report, or the same refusal, with the entries inflated on threads and the parts parsed ahead of the read.
#![no_main]

use diacraft_vsdx::{Limits, read_partial};
use diacraft_vsdx_fuzz::{checks, inputs};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(input) = inputs::input(data) else { return };
    let limits = Limits::default();
    let (result, report) = read_partial(&input.bytes, &limits);
    // The same read on threads (the count is the input's length, 2 to 5): the same document and report, or the same refusal, whatever the bytes are.
    let threads = 2 + data.len() % 4;
    let (on_threads, threaded_report) = read_partial(&input.bytes, &Limits { threads, ..limits.clone() });
    assert!(on_threads == result && threaded_report == report, "a read on {threads} threads is not the read on one");
    checks::warnings_in_words(&report);
    assert!(report.stats.memory_peak <= limits.max_memory, "the pool was passed: {:?}", report.stats);
    match result {
        Ok(doc) => {
            doc.check_limits().expect("a document that read passes the model's own caps");
            checks::held_is_the_packages(&input.bytes, &doc, &report);
            checks::carried_parts_are_sound(&input.bytes, &doc, &report);
            if data.len().is_multiple_of(4) {
                let (again, second) = read_partial(&input.bytes, &limits);
                assert!(again.as_ref().is_ok_and(|d| *d == doc) && second == report, "reading twice gave two documents");
            }
        }
        Err(e) => checks::refusal_in_words(&e),
    }
    if let Some(pool) = input.pool {
        checks::read_in_a_pool(&input.bytes, pool);
    }
});
