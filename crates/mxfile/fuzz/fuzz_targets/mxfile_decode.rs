//! `cargo fuzz run mxfile_decode`: bytes as the text of a compressed page, as the XML of a page to encode and decode again, as the steps of R11 one by one, or as a whole file
//! Through the pipeline with the real caps.
//!
//! How the input is read, and the properties it is held to, are the library's (`src/lib.rs`), which the reader's tests include: no panic, no hang, the work never past its
//! budget whether the read gives a drawing or a refusal, the same answer twice, what the encoder encodes the decoder decodes, and a drawing that holds the model's caps.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| diacraft_mxfile_fuzz::check(data));
