//! `cargo fuzz run mxfile_label`: bytes as a draw.io label, plain or HTML, as pieces a recipe chooses (tags, references, styles, white space, and what a hostile label is made of), and with
//! the caps of the read made small, through the reader of labels.
//!
//! How the input is read, and the properties it is held to, are in `src/label.rs`, which this target and the reader's tests both include by path: no panic, no hang, the work never past its budget whether
//! the read gives a label or a refusal and at most 40 steps for each byte, the same answer twice, no empty run and no two runs alike, no character the clean takes out, a size that is
//! finite and above 0, and a label past the cap cut and counted.
#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../src/label.rs"]
mod label;

fuzz_target!(|data: &[u8]| label::check(data));
