//! `cargo fuzz run canon_compare`: the content check on two mutated copies of one package (done-criterion D5).
//!
//! How the input is read, and the properties it is held to, are the library's (`src/lib.rs`): the container's tests include that file and replay the inputs this target crashed
//! on through the same law (`crates/container/tests/it/canon_fuzz.rs`). The count is symmetric where nothing orients it; the one rule that orients the check
//! `ignored_when_extra`, is held to its own law.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| diacraft_container_fuzz::check(data));
