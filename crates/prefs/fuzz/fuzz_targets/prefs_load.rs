//! `cargo fuzz run prefs_load`: the person's settings read from bytes: the text of `config.toml`, a word list, a language tag, a file name.
//!
//! How the input is read and the properties it is held to are the library's (`src/lib.rs`), which the crate's tests include: no panic, no hang, every list in range, what is
//! brought into range stays so, and what is saved is read back as it was.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| diacraft_prefs_fuzz::check(data));
