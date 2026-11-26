//! `cargo fuzz run mxfile_read`: bytes as a draw.io file, as the cells of a plain page, or as a recipe of cells that collide on purpose, read with the real limits and with small ones
//! The file, the pages and the cells.
//!
//! How the input is read, and the properties it is held to, are the library's (`src/read.rs`), which the reader's tests include: no panic, no hang, the work never past its budget
//! whether the read gives a drawing or a refusal, the same answer twice, and a graph of cells that holds every invariant of the tree (one root, each cell reached once, depths,
//! unique ids, roles that agree with the flags, numbers within the cap).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| diacraft_mxfile_fuzz::read::check(data));
