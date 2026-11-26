//! `cargo fuzz run mxfile_style`: bytes as a style string, a colour value, a number or a list of styles, through the style reader with the real caps.
//!
//! How the input is read, and the properties it is held to, are in `src/style.rs`, which this target and the reader's tests include by its path: no panic, no hang, the work never
//! past its budget, the same answer twice, every number finite and in range, every value what its key's type says, a colour that writes itself out and reads back as itself, and a
//! style parsed once and not again.
#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../src/style.rs"]
mod style;

fuzz_target!(|data: &[u8]| style::check(data));
